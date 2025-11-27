// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::PeerInfo;
use crate::app::TorrentMetrics;

use crate::resource_manager::ResourceManagerClient;
use crate::resource_manager::ResourceManagerError;

use crate::networking::web_seed_worker::web_seed_worker;
use crate::networking::ConnectionType;

use crate::token_bucket::TokenBucket;

use crate::torrent_manager::DiskIoOperation;

use crate::config::Settings;

use crate::torrent_manager::piece_manager::PieceStatus;

use crate::torrent_manager::state::Action;
use crate::torrent_manager::state::ChokeStatus;
use crate::torrent_manager::state::Effect;
use crate::torrent_manager::state::PeerState;
use crate::torrent_manager::state::TorrentActivity;
use crate::torrent_manager::state::TorrentState;

use crate::torrent_manager::piece_manager::PieceManager;
use crate::torrent_manager::state::TorrentStatus;
use crate::torrent_manager::state::TrackerState;
use crate::torrent_manager::ManagerCommand;
use crate::torrent_manager::ManagerEvent;

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
    in_flight_writes: HashMap<u32, Vec<JoinHandle<()>>>,

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

        event!(Level::INFO, "Added new torrent {:?}", torrent);

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
            in_flight_writes: HashMap::new(),
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
            in_flight_writes: HashMap::new(),
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
                self.send_metrics(bytes_dl, bytes_ul);
            }

            Effect::SendToPeer { peer_id, cmd } => {
                if let Some(peer) = self.state.peers.get(&peer_id) {
                    let _ = peer.peer_tx.try_send(*cmd);
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
                    )
                    .await;
                });
            }

            Effect::DisconnectPeer { peer_id } => {
                if let Some(peer) = self.state.peers.get(&peer_id) {
                    let _ = peer
                        .peer_tx
                        .try_send(TorrentCommand::Disconnect(peer_id.clone()));
                }
                if let Some(handles) = self.in_flight_uploads.remove(&peer_id) {
                    for handle in handles.values() {
                        handle.abort();
                    }
                }
            }

            Effect::BroadcastHave { piece_index } => {
                for peer in self.state.peers.values() {
                    let _ = peer
                        .peer_tx
                        .try_send(TorrentCommand::Have(peer.ip_port.clone(), piece_index));
                }
            }

            Effect::VerifyPiece {
                peer_id,
                piece_index,
                data,
            } => {
                let torrent = match self.state.torrent.clone() {
                    Some(t) => t,
                    None => {
                        debug_assert!(
                            self.state.torrent.is_some(),
                            "Metadata missing during verify"
                        );
                        event!(
                            Level::ERROR,
                            "Metadata missing during piece verification, cannot proceed."
                        );
                        return;
                    }
                };
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
                            let _ = tx
                                .send(TorrentCommand::PieceVerified {
                                    piece_index,
                                    peer_id: peer_id_for_msg,
                                    verification_result: Ok(verified_data),
                                })
                                .await;
                        }
                        _ => {
                            let _ = tx
                                .send(TorrentCommand::PieceVerified {
                                    piece_index,
                                    peer_id: peer_id_for_msg,
                                    verification_result: Err(()),
                                })
                                .await;
                        }
                    }
                });
            }

            Effect::WriteToDisk {
                peer_id,
                piece_index,
                data,
            } => {
                let multi_file_info = match self.multi_file_info.as_ref() {
                    Some(m) => m.clone(),
                    None => {
                        event!(Level::ERROR, "WriteToDisk failed: Storage not ready");
                        return;
                    }
                };
                let piece_length = match self.state.torrent.as_ref() {
                    Some(t) => t.info.piece_length as u64,
                    None => {
                        event!(Level::ERROR, "WriteToDisk failed: Metadata missing");
                        return;
                    }
                };

                let global_offset = piece_index as u64 * piece_length;

                let tx = self.torrent_manager_tx.clone();
                let event_tx = self.manager_event_tx.clone();
                let resource_manager = self.resource_manager.clone();
                let info_hash = self.state.info_hash.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                let peer_id_clone = peer_id.clone();

                let handle = tokio::spawn(async move {
                    let op = DiskIoOperation {
                        piece_index,
                        offset: global_offset,
                        length: data.len(),
                    };

                    let result = Self::write_block_with_retry(
                        &multi_file_info,
                        &resource_manager,
                        &mut shutdown_rx,
                        &event_tx,
                        &info_hash,
                        op,
                        &data,
                    )
                    .await;

                    match result {
                        Ok(_) => {
                            let _ = tx
                                .send(TorrentCommand::PieceWrittenToDisk {
                                    peer_id: peer_id_clone,
                                    piece_index,
                                })
                                .await;
                        }
                        Err(e) => {
                            event!(
                                Level::ERROR,
                                "Write failed for piece {}: {}",
                                piece_index,
                                e
                            );
                            let _ = tx
                                .send(TorrentCommand::PieceWriteFailed { piece_index })
                                .await;
                        }
                    }
                });

                self.in_flight_writes
                    .entry(piece_index)
                    .or_default()
                    .push(handle);
            }

            Effect::ReadFromDisk {
                peer_id,
                block_info,
            } => {
                let (peer_semaphore, peer_tx) = if let Some(peer) = self.state.peers.get(&peer_id) {
                    (peer.upload_slots_semaphore.clone(), peer.peer_tx.clone())
                } else {
                    return;
                };

                let _peer_permit = match peer_semaphore.try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => return,
                };

                let multi_file_info = match self.multi_file_info.as_ref() {
                    Some(m) => m.clone(),
                    None => {
                        event!(Level::ERROR, "WriteToDisk failed: Storage not ready");
                        return;
                    }
                };

                let torrent = match self.state.torrent.as_ref() {
                    Some(t) => t,
                    None => {
                        event!(
                            Level::ERROR,
                            "ReadFromDisk triggered but metadata missing. Ignoring."
                        );
                        return;
                    }
                };

                let global_offset = (block_info.piece_index as u64
                    * torrent.info.piece_length as u64)
                    + block_info.offset as u64;

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
                        piece_index: block_info.piece_index,
                        offset: global_offset,
                        length: block_info.length as usize,
                    };

                    let _ = event_tx.try_send(ManagerEvent::DiskReadStarted {
                        info_hash: info_hash.to_vec(),
                        op,
                    });

                    let result = Self::read_block_with_retry(
                        &multi_file_info,
                        &resource_manager,
                        &mut shutdown_rx,
                        &event_tx,
                        op,
                        &peer_tx,
                    )
                    .await;

                    if let Ok(data) = result {
                        let _ = peer_tx.try_send(TorrentCommand::Upload(
                            block_info.piece_index,
                            block_info.offset,
                            data,
                        ));

                        let _ = tx.try_send(TorrentCommand::UploadTaskCompleted {
                            peer_id: peer_id_clone.clone(),
                            block_info: block_info_clone,
                        });

                        let _ = tx
                            .send(TorrentCommand::BlockSent {
                                peer_id: peer_id_clone.clone(),
                                bytes: block_info.length as u64,
                            })
                            .await;
                    }

                    let _ = event_tx.try_send(ManagerEvent::DiskReadFinished);
                });

                self.in_flight_uploads
                    .entry(peer_id)
                    .or_default()
                    .insert(block_info, handle);
            }

            Effect::ConnectToPeer { ip, port } => {
                let peer_ip_port = format!("{}:{}", ip, port);

                if self.state.peers.contains_key(&peer_ip_port) {
                    return;
                }

                let manager_tx = self.torrent_manager_tx.clone();
                let rm = self.resource_manager.clone();
                let dl_bucket = self.global_dl_bucket.clone();
                let ul_bucket = self.global_ul_bucket.clone();
                let info_hash = self.state.info_hash.clone();
                let meta_len = self.state.torrent_metadata_length;
                let client_id = self.settings.client_id.clone();
                let shutdown_tx = self.shutdown_tx.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();

                let (peer_tx, peer_rx) = mpsc::channel(10);
                self.state.peers.insert(
                    peer_ip_port.clone(),
                    PeerState::new(peer_ip_port.clone(), peer_tx, Instant::now()),
                );

                let bitfield = if self.state.torrent.is_some() {
                    Some(self.generate_bitfield())
                } else {
                    None
                };

                tokio::spawn(async move {
                    let permit = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => None,
                        res = rm.acquire_peer_connection() => res.ok()
                    };

                    if let Some(p) = permit {
                        if let Ok(Ok(stream)) =
                            timeout(Duration::from_secs(2), TcpStream::connect(&peer_ip_port)).await
                        {
                            let _held = p;
                            let session = PeerSession::new(PeerSessionParameters {
                                info_hash,
                                torrent_metadata_length: meta_len,
                                connection_type: ConnectionType::Outgoing,
                                torrent_manager_rx: peer_rx,
                                torrent_manager_tx: manager_tx.clone(),
                                peer_ip_port: peer_ip_port.clone(),
                                client_id: client_id.into(),
                                global_dl_bucket: dl_bucket,
                                global_ul_bucket: ul_bucket,
                                shutdown_tx,
                            });

                            let _ = session.run(stream, Vec::new(), bitfield).await;
                        } else {
                            let _ = manager_tx
                                .send(TorrentCommand::UnresponsivePeer(peer_ip_port.clone()))
                                .await;
                        }
                    }
                    let _ = manager_tx
                        .send(TorrentCommand::Disconnect(peer_ip_port))
                        .await;
                });
            }

            Effect::InitializeStorage => {
                if let Some(torrent) = &self.state.torrent {
                    let mfi = match MultiFileInfo::new(
                        &self.root_download_path,
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
                    ) {
                        Ok(mfi) => mfi,
                        Err(e) => {
                            debug_assert!(false, "Failed to init storage from metadata: {}", e);
                            event!(Level::ERROR, "Failed to initialize storage from metadata: {}. Aborting storage initialization.", e);
                            return;
                        }
                    };

                    self.multi_file_info = Some(mfi.clone());
                }
            }

            Effect::StartValidation => {
                let mfi = match self.multi_file_info.as_ref() {
                    Some(m) => m.clone(),
                    None => {
                        debug_assert!(
                            self.multi_file_info.is_some(),
                            "Storage not ready for validation"
                        );
                        event!(
                            Level::ERROR,
                            "Cannot start validation: Storage not initialized."
                        );
                        return;
                    }
                };
                let torrent = match self.state.torrent.as_ref() {
                    Some(t) => t.clone(),
                    None => {
                        debug_assert!(
                            self.state.torrent.is_some(),
                            "Metadata not ready for validation"
                        );
                        event!(
                            Level::ERROR,
                            "Cannot start validation: Metadata not available."
                        );
                        return;
                    }
                };
                let rm = self.resource_manager.clone();
                let shutdown_rx = self.shutdown_tx.subscribe();
                let event_tx = self.manager_event_tx.clone();
                let manager_tx = self.torrent_manager_tx.clone();

                let is_validated = self.state.torrent_validation_status;

                tokio::spawn(async move {
                    let res = Self::perform_validation(
                        mfi,
                        torrent,
                        rm,
                        shutdown_rx,
                        manager_tx.clone(),
                        event_tx,
                        is_validated,
                    )
                    .await;

                    if let Ok(pieces) = res {
                        let _ = manager_tx
                            .send(TorrentCommand::ValidationComplete(pieces))
                            .await;
                    }
                });
            }

            Effect::ConnectToPeersFromTrackers => {
                let torrent_size_left = self
                    .multi_file_info
                    .as_ref()
                    .map_or(0, |mfi| mfi.total_size as usize);

                for url in self.state.trackers.keys() {
                    let tx = self.torrent_manager_tx.clone();
                    let url_clone = url.clone();
                    let info_hash = self.state.info_hash.clone();
                    let port = self.settings.client_port;
                    let client_id = self.settings.client_id.clone();
                    let mut shutdown_rx = self.shutdown_tx.subscribe();

                    tokio::spawn(async move {
                        let response = tokio::select! {
                            res = announce_started(
                                url_clone.clone(),
                                &info_hash,
                                client_id,
                                port,
                                torrent_size_left,
                            ) => res,
                            _ = shutdown_rx.recv() => return
                        };

                        match response {
                            Ok(resp) => {
                                let _ = tx
                                    .send(TorrentCommand::AnnounceResponse(url_clone, resp))
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(TorrentCommand::AnnounceFailed(url_clone, e.to_string()))
                                    .await;
                            }
                        }
                    });
                }
            }

            Effect::AnnounceToTracker { url } => {
                let info_hash = self.state.info_hash.clone();
                let client_id = self.settings.client_id.clone();
                let port = self.settings.client_port;
                let ul = self.state.session_total_uploaded as usize;
                let dl = self.state.session_total_downloaded as usize;

                let torrent_size_left = if let Some(mfi) = &self.multi_file_info {
                    let completed = self
                        .state
                        .piece_manager
                        .bitfield
                        .iter()
                        .filter(|&&s| s == PieceStatus::Done)
                        .count();
                    let piece_len = self
                        .state
                        .torrent
                        .as_ref()
                        .map(|t| t.info.piece_length)
                        .unwrap_or(0) as u64;
                    let completed_bytes = (completed as u64) * piece_len;
                    mfi.total_size.saturating_sub(completed_bytes) as usize
                } else {
                    0
                };

                let tx = self.torrent_manager_tx.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();

                tokio::spawn(async move {
                    let res = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => return,
                        r = announce_periodic(
                            url.clone(),
                            &info_hash,
                            client_id,
                            port,
                            ul,
                            dl,
                            torrent_size_left
                        ) => r
                    };

                    match res {
                        Ok(resp) => {
                            let _ = tx.send(TorrentCommand::AnnounceResponse(url, resp)).await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(TorrentCommand::AnnounceFailed(url, e.to_string()))
                                .await;
                        }
                    }
                });
            }

            Effect::AbortUpload {
                peer_id,
                block_info,
            } => {
                if let Some(peer_uploads) = self.in_flight_uploads.get_mut(&peer_id) {
                    if let Some(handle) = peer_uploads.remove(&block_info) {
                        handle.abort();
                        event!(Level::TRACE, peer = %peer_id, ?block_info, "Aborted in-flight upload task.");
                    }
                }
            }

            Effect::ClearAllUploads => {
                for (_, handles) in self.in_flight_uploads.drain() {
                    for (_, handle) in handles {
                        handle.abort();
                    }
                }
            }

            Effect::TriggerDhtSearch => {
                #[cfg(feature = "dht")]
                let _ = self.dht_trigger_tx.send(());
            }

            Effect::DeleteFiles => {
                let info_hash = self.state.info_hash.clone();
                let root_path = self.root_download_path.clone();
                let multi_file_info = self.multi_file_info.clone();
                let tx = self.manager_event_tx.clone();
                let torrent_name = self.state.torrent.as_ref().map(|t| t.info.name.clone());

                tokio::spawn(async move {
                    let mut result = Ok(());

                    if let Some(mfi) = multi_file_info {
                        for file_info in &mfi.files {
                            if let Err(e) = fs::remove_file(&file_info.path).await {
                                if e.kind() != std::io::ErrorKind::NotFound {
                                    let error_msg = format!(
                                        "Failed to delete file {:?}: {}",
                                        &file_info.path, e
                                    );
                                    event!(Level::ERROR, "{}", error_msg);
                                    result = Err(error_msg);
                                }
                            }
                        }

                        if result.is_ok() && mfi.files.len() > 1 {
                            if let Some(name) = torrent_name {
                                let content_dir = root_path.join(name);
                                event!(Level::INFO, "Cleaning up directory: {:?}", &content_dir);
                                let _ = fs::remove_dir(&content_dir).await;
                            }
                        }
                    } else {
                        let msg = "Cannot delete files: Metadata missing.".to_string();
                        event!(Level::WARN, "{}", msg);
                        result = Err(msg);
                    }

                    let _ = tx
                        .send(ManagerEvent::DeletionComplete(info_hash, result))
                        .await;
                });
            }

            Effect::PrepareShutdown {
                tracker_urls,
                left,
                uploaded,
                downloaded,
            } => {
                let _ = self.shutdown_tx.send(());

                event!(Level::DEBUG, "Aborting in-flight upload/write tasks...");
                for (_, handles) in self.in_flight_uploads.drain() {
                    for (_, handle) in handles {
                        handle.abort();
                    }
                }
                for (_, handles) in self.in_flight_writes.drain() {
                    for handle in handles {
                        handle.abort();
                    }
                }

                let mut announce_set = JoinSet::new();
                for url in tracker_urls {
                    let info_hash = self.state.info_hash.clone();
                    let port = self.settings.client_port;
                    let client_id = self.settings.client_id.clone();

                    announce_set.spawn(async move {
                        announce_stopped(
                            url, &info_hash, client_id, port, uploaded, downloaded, left,
                        )
                        .await;
                    });
                }

                let tx = self.manager_event_tx.clone();
                let info_hash = self.state.info_hash.clone();

                tokio::spawn(async move {
                    if (timeout(Duration::from_secs(4), async {
                        while announce_set.join_next().await.is_some() {}
                    })
                    .await)
                        .is_err()
                    {
                        event!(Level::WARN, "Tracker stop announce timed out.");
                    }
                    let _ = tx
                        .send(ManagerEvent::DeletionComplete(info_hash, Ok(())))
                        .await;
                });
            }

            Effect::StartWebSeed { url } => {
                let (full_url, _filename) = if let Some(torrent) = &self.state.torrent {
                    if url.ends_with('/') {
                        (format!("{}{}", url, torrent.info.name), torrent.info.name.clone())
                    } else {
                        (url.clone(), torrent.info.name.clone())
                    }
                } else {
                    event!(Level::WARN, "Triggered StartWebSeed but metadata is missing. Skipping.");
                    return;
                };

                let torrent_manager_tx = self.torrent_manager_tx.clone();
                let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(32);

                // Use the full URL as the peer_id so the worker and manager agree on the ID
                let peer_id = full_url.clone();

                self.apply_action(Action::RegisterPeer {
                    peer_id: peer_id.clone(),
                    tx: peer_tx,
                });

                let shutdown_rx = self.shutdown_tx.subscribe();

                if let Some(torrent) = &self.state.torrent {
                    let piece_len = torrent.info.piece_length as u64;
                    
                    // Calculate total length robustly (handle multi-file vs single-file)
                    let total_len = if torrent.info.files.is_empty() {
                        torrent.info.length as u64
                    } else {
                         torrent.info.files.iter().map(|f| f.length as u64).sum()
                    };

                    event!(Level::INFO, "Starting WebSeed Worker: {}", full_url);

                    tokio::spawn(async move {
                        web_seed_worker(
                            full_url, 
                            peer_id,
                            piece_len,
                            total_len,
                            peer_rx,
                            torrent_manager_tx,
                            shutdown_rx,
                        )
                        .await;
                    });
                }
            }
        }
    }

    async fn perform_validation(
        multi_file_info: MultiFileInfo,
        torrent: Torrent,
        resource_manager: ResourceManagerClient,
        mut shutdown_rx: broadcast::Receiver<()>,
        manager_tx: Sender<TorrentCommand>,
        event_tx: Sender<ManagerEvent>,
        skip_hashing: bool,
    ) -> Result<Vec<u32>, StorageError> {
        let num_pieces = torrent.info.pieces.len() / 20;
        if skip_hashing {
            event!(
                Level::INFO,
                "Torrent already validated. Skipping hash check."
            );
            let all_pieces: Vec<u32> = (0..num_pieces).map(|i| i as u32).collect();
            return Ok(all_pieces);
        }

        let mut completed_pieces = Vec::new();

        tokio::select! {
            biased;
            _ = shutdown_rx.recv() => {
                return Err(StorageError::Io(std::io::Error::other("Shutdown during allocation")));
            }
            res = create_and_allocate_files(&multi_file_info) => res?,
        };

        let piece_length_u64 = torrent.info.piece_length as u64;
        let total_size = multi_file_info.total_size;

        for piece_index in 0..num_pieces {
            let start_offset = (piece_index as u64) * piece_length_u64;
            let len_this_piece =
                std::cmp::min(piece_length_u64, total_size.saturating_sub(start_offset)) as usize;

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
                    _ = shutdown_rx.recv() => return Ok(completed_pieces),
                    res = resource_manager.acquire_disk_read() => res
                };

                match disk_permit_result {
                    Ok(_permit) => {
                        let read_result = tokio::select! {
                            biased;
                            _ = shutdown_rx.recv() => return Ok(completed_pieces),
                            res = read_data_from_disk(&multi_file_info, start_offset, len_this_piece) => res
                        };

                        match read_result {
                            Ok(data) => break data,
                            Err(e) => {
                                event!(Level::WARN, piece = piece_index, error = %e, "Read failed during validation.");
                            }
                        }
                    }
                    Err(ResourceManagerError::QueueFull) => { /* Retry */ }
                    Err(ResourceManagerError::ManagerShutdown) => return Ok(completed_pieces),
                }

                if attempt >= MAX_VALIDATION_ATTEMPTS {
                    event!(
                        Level::ERROR,
                        piece = piece_index,
                        "Validation read failed after max attempts."
                    );
                    return Ok(completed_pieces);
                }

                let backoff = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
                let jitter = rand::rng().random_range(0..=JITTER_MS);
                attempt += 1;

                let _ = event_tx.try_send(ManagerEvent::DiskIoBackoff {
                    duration: Duration::from_millis(backoff + jitter),
                });

                if Self::sleep_with_shutdown(
                    Duration::from_millis(backoff + jitter),
                    &mut shutdown_rx,
                )
                .await
                .is_err()
                {
                    return Ok(completed_pieces);
                }
            };

            if piece_data.is_empty() {
                continue;
            }

            let mut validation_task = tokio::task::spawn_blocking(move || {
                if let Some(expected) = expected_hash {
                    sha1::Sha1::digest(&piece_data).as_slice() == expected.as_slice()
                } else {
                    false
                }
            });

            let is_valid = tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    validation_task.abort();
                    return Ok(completed_pieces);
                }
                res = &mut validation_task => res.unwrap_or(false)
            };

            if is_valid {
                completed_pieces.push(piece_index as u32);
            }

            if piece_index % 20 == 0 {
                let _ = manager_tx
                    .send(TorrentCommand::ValidationProgress(piece_index as u32))
                    .await;
            }
        }

        let _ = manager_tx
            .send(TorrentCommand::ValidationProgress(num_pieces as u32))
            .await;
        Ok(completed_pieces)
    }

    async fn write_block_with_retry(
        multi_file_info: &MultiFileInfo,
        resource_manager: &ResourceManagerClient,
        shutdown_rx: &mut broadcast::Receiver<()>,
        event_tx: &Sender<ManagerEvent>,
        info_hash: &[u8],
        op: DiskIoOperation,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let mut attempt = 0;
        let _ = event_tx.try_send(ManagerEvent::DiskWriteStarted {
            info_hash: info_hash.to_vec(),
            op,
        });

        loop {
            let permit_res = tokio::select! {
                biased;
                _ = shutdown_rx.recv() => return Err(StorageError::Io(std::io::Error::other("Shutdown"))),
                res = resource_manager.acquire_disk_write() => res,
            };

            match permit_res {
                Ok(_permit) => {
                    let write_future = write_data_to_disk(multi_file_info, op.offset, data);
                    let res = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => return Err(StorageError::Io(std::io::Error::other("Shutdown"))),
                        r = write_future => r,
                    };

                    match res {
                        Ok(_) => {
                            let _ = event_tx.try_send(ManagerEvent::DiskWriteFinished);
                            return Ok(());
                        }
                        Err(e) => {
                            event!(Level::WARN, piece = op.piece_index, error = ?e, "Disk write failed (IO Error).");
                        }
                    }
                }
                Err(ResourceManagerError::ManagerShutdown) => {
                    return Err(StorageError::Io(std::io::Error::other("Manager Shutdown")))
                }
                Err(ResourceManagerError::QueueFull) => {
                    event!(
                        Level::WARN,
                        piece = op.piece_index,
                        "Disk write queue full (Permit Starvation)."
                    );
                }
            }

            attempt += 1;
            if attempt > MAX_PIECE_WRITE_ATTEMPTS {
                let _ = event_tx.try_send(ManagerEvent::DiskWriteFinished);
                return Err(StorageError::Io(std::io::Error::other(
                    "Max write attempts exceeded",
                )));
            }

            let backoff = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
            let jitter = rand::rng().random_range(0..=JITTER_MS);
            let duration = Duration::from_millis(backoff + jitter);
            event!(
                Level::WARN,
                piece = op.piece_index,
                attempt = attempt,
                duration_ms = duration.as_millis(),
                "Retrying disk write..."
            );

            let _ = event_tx.try_send(ManagerEvent::DiskIoBackoff {
                duration: Duration::from_millis(backoff + jitter),
            });

            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => return Err(StorageError::Io(std::io::Error::other("Shutdown"))),
                _ = tokio::time::sleep(Duration::from_millis(backoff + jitter)) => {},
            }
        }
    }

    async fn read_block_with_retry(
        multi_file_info: &MultiFileInfo,
        resource_manager: &ResourceManagerClient,
        shutdown_rx: &mut broadcast::Receiver<()>,
        event_tx: &Sender<ManagerEvent>,
        op: DiskIoOperation,
        peer_tx: &Sender<TorrentCommand>,
    ) -> Result<Vec<u8>, StorageError> {
        let mut attempt = 0;

        loop {
            if peer_tx.is_closed() {
                return Err(StorageError::Io(std::io::Error::other("Peer Disconnected")));
            }

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
                            return Ok(data);
                        }
                        Err(e) => {
                            event!(Level::WARN, piece = op.piece_index, error = ?e, "Disk read failed (IO Error).");
                        }
                    }
                }
                Err(ResourceManagerError::ManagerShutdown) => {
                    return Err(StorageError::Io(std::io::Error::other("Manager Shutdown")))
                }
                Err(ResourceManagerError::QueueFull) => {
                    event!(
                        Level::WARN,
                        piece = op.piece_index,
                        "Disk read queue full (Permit Starvation)."
                    );
                }
            }

            attempt += 1;
            if attempt > MAX_UPLOAD_REQUEST_ATTEMPTS {
                return Err(StorageError::Io(std::io::Error::other(
                    "Max read attempts exceeded",
                )));
            }

            let backoff = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
            let jitter = rand::rng().random_range(0..=JITTER_MS);
            let duration = Duration::from_millis(backoff + jitter);

            event!(
                Level::WARN,
                piece = op.piece_index,
                attempt = attempt,
                duration_ms = duration.as_millis(),
                "Retrying disk read..."
            );

            let _ = event_tx.try_send(ManagerEvent::DiskIoBackoff { duration });

            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => { return Err(StorageError::Io(std::io::Error::other("Shutdown"))); }
                _ = tokio::time::sleep(duration) => {},
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

        if let Some((failure_count, next_attempt_time)) =
            self.state.timed_out_peers.get(&peer_ip_port)
        {
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
        self.apply_action(Action::RegisterPeer {
            peer_id: peer_ip_port.clone(),
            tx: peer_session_tx,
        });

        let bitfield = match self.state.torrent {
            None => None,
            _ => Some(self.generate_bitfield()),
        };

        let client_id_clone = self.settings.client_id.clone();
        tokio::spawn(async move {
            let session_permit = tokio::select! {
                permit_result = timeout(Duration::from_secs(10), resource_manager_clone.acquire_peer_connection()) => {
                    match permit_result {
                        Ok(Ok(permit)) => Some(permit), // Acquired
                        _ => None, // Timeout or Manager Shutdown
                    }
                }
                _ = shutdown_rx_permit.recv() => {
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

    pub async fn validate_local_file(&mut self) -> Result<(), StorageError> {
        let mfi = match &self.multi_file_info {
            Some(i) => i.clone(),
            None => return Ok(()),
        };

        // We can safely expect metadata here because this is called on startup
        // for existing torrents, which must have metadata to exist.
        let torrent = match self.state.torrent.clone() {
            Some(t) => t,
            None => {
                debug_assert!(
                    self.state.torrent.is_some(),
                    "Metadata missing during startup validation"
                );
                event!(
                    Level::ERROR,
                    "Cannot validate local file: Metadata not available."
                );
                return Err(StorageError::Io(std::io::Error::other(
                    "Metadata missing during startup validation",
                )));
            }
        };

        let rm = self.resource_manager.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        let manager_tx = self.torrent_manager_tx.clone();
        let event = self.manager_event_tx.clone();
        let skip = self.state.torrent_validation_status;

        tokio::spawn(async move {
            let result = Self::perform_validation(
                mfi,
                torrent,
                rm,
                shutdown_rx,
                manager_tx.clone(),
                event,
                skip,
            )
            .await;

            match result {
                Ok(pieces) => {
                    let _ = manager_tx
                        .send(TorrentCommand::ValidationComplete(pieces))
                        .await;
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    event!(Level::ERROR, "Triggering Fatal Pause due to: {}", error_msg);
                    let _ = manager_tx
                        .send(TorrentCommand::FatalStorageError(error_msg))
                        .await;
                }
            }
        });

        Ok(())
    }

    fn generate_activity_message(&self, dl_speed: u64, ul_speed: u64) -> String {
        if self.state.is_paused {
            return "Paused".to_string();
        }

        if self.state.torrent_status == TorrentStatus::AwaitingMetadata {
            return "Retrieving Metadata...".to_string();
        }

        if self.state.torrent_status == TorrentStatus::Validating {
            return "Validating...".to_string();
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

    fn send_metrics(&mut self, bytes_dl: u64, bytes_ul: u64) {
        if let Some(ref torrent) = self.state.torrent {
            let multi_file_info = match self.multi_file_info.as_ref() {
                Some(mfi) => mfi,
                None => {
                    debug_assert!(
                        self.multi_file_info.is_some(),
                        "File info not ready for metrics."
                    );
                    event!(Level::WARN, "Cannot send metrics: File info not available.");
                    return;
                }
            };

            let next_announce_in = self
                .state
                .trackers
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
                if self.state.torrent_status == TorrentStatus::Validating {
                    self.state.validation_pieces_found
                } else {
                    number_of_pieces_total - self.state.piece_manager.pieces_remaining as u32
                };

            let number_of_successfully_connected_peers = self.state.peers.len();

            let eta = if self.state.piece_manager.pieces_remaining == 0 {
                Duration::from_secs(0)
            } else if smoothed_total_dl_speed == 0 {
                Duration::MAX
            } else {
                let total_size_bytes = multi_file_info.total_size;
                let bytes_completed = (torrent.info.piece_length as u64).saturating_mul(
                    self.state
                        .piece_manager
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
                .state
                .peers
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
        // 1. Magnet (No Metadata): announce_immediately = true.
        //    We MUST find peers to get metadata.
        // 2. File (Has Metadata): announce_immediately = false.
        //    We wait for validation to finish so we report accurate "Left" stats
        //    to the tracker (preventing bans on private trackers).
        let announce_immediately = self.state.torrent.is_none();
        self.apply_action(Action::TorrentManagerInit {
            is_paused,
            announce_immediately,
        });

        if self.state.torrent.is_some() {
            if let Err(error) = self.validate_local_file().await {
                match error {
                    StorageError::Io(e) => {
                        eprintln!("Error calling validate local file: {}", e);
                    }
                }
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
                    self.apply_action(Action::Cleanup);
                }
                _ = tick.tick(), if !self.state.is_paused => {

                    let now = Instant::now();
                    let actual_duration = now.duration_since(last_tick_time);
                    last_tick_time = now;
                    let actual_ms = actual_duration.as_millis() as u64;

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
                        random_seed: rand::rng().random()
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
                        ManagerCommand::Pause => self.apply_action(Action::Pause),
                        ManagerCommand::Resume => self.apply_action(Action::Resume),
                        ManagerCommand::DeleteFile => {
                            self.apply_action(Action::Delete);
                            break Ok(());
                        },
                        ManagerCommand::UpdateListenPort(new_port) => {
                            let mut settings = (*self.settings).clone();
                            if settings.client_port != new_port {
                                settings.client_port = new_port;
                                self.settings = Arc::new(settings);
                                self.apply_action(Action::UpdateListenPort);
                            }

                        },
                        ManagerCommand::SetDataRate(new_rate_ms) => {
                            data_rate_ms = new_rate_ms;
                            tick = tokio::time::interval(Duration::from_millis(data_rate_ms));
                            tick.reset();
                            last_tick_time = Instant::now();
                        },
                        ManagerCommand::Shutdown => {
                            self.apply_action(Action::Shutdown);
                            break Ok(());
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

                        self.apply_action(Action::RegisterPeer {
                            peer_id: peer_ip_port.clone(),
                            tx: peer_session_tx,
                        });

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
                        TorrentCommand::PeerId(addr, id) => self.apply_action(Action::UpdatePeerId { peer_addr: addr, new_id: id }),
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

                        TorrentCommand::PieceWrittenToDisk { peer_id, piece_index } => {
                            if let Some(handles) = self.in_flight_writes.remove(&piece_index) {
                                for handle in handles {
                                    handle.abort();
                                }
                            }
                            self.apply_action(Action::PieceWrittenToDisk { peer_id, piece_index });
                        },
                        TorrentCommand::PieceWriteFailed { piece_index } => {
                            if let Some(handles) = self.in_flight_writes.remove(&piece_index) {
                                for handle in handles {
                                    handle.abort();
                                }
                            }
                            self.apply_action(Action::PieceWriteFailed { piece_index });
                        },
                        TorrentCommand::RequestUpload(peer_id, piece_index, block_offset, block_length) => self.apply_action(Action::RequestUpload { peer_id, piece_index, block_offset, length: block_length }),
                        TorrentCommand::CancelUpload(peer_id, piece_index, block_offset, block_length) => {
                            self.apply_action(Action::CancelUpload {
                                peer_id,
                                piece_index,
                                block_offset,
                                length: block_length
                            });
                        },
                        TorrentCommand::UploadTaskCompleted { peer_id, block_info } => {
                            if let Some(peer_uploads) = self.in_flight_uploads.get_mut(&peer_id) {
                                peer_uploads.remove(&block_info);
                            }
                        },

                        TorrentCommand::DhtTorrent(torrent, metadata_length) => {
                            #[cfg(all(feature = "dht", feature = "pex"))]
                            if torrent.info.private == Some(1) {
                                break Ok(());
                            }

                            let mut hasher = Sha1::new();
                            hasher.update(&torrent.info_dict_bencode);
                            if hasher.finalize().as_slice() != self.state.info_hash.as_slice() {
                                continue;
                            }

                            self.apply_action(Action::MetadataReceived {
                                torrent: Box::new(torrent), metadata_length
                            });
                        },

                        TorrentCommand::AnnounceResponse(url, response) => {
                            self.apply_action(Action::TrackerResponse {
                                url,
                                peers: response.peers.into_iter().map(|p| (p.ip, p.port)).collect(),
                                interval: response.interval as u64,
                                min_interval: response.min_interval.map(|i| i as u64)
                            });
                        },

                        TorrentCommand::AnnounceFailed(url, error) => {
                            event!(Level::DEBUG, "Error from tracker announced failed {}", error);
                            self.apply_action(Action::TrackerError { url });
                        },

                        TorrentCommand::UnresponsivePeer(peer_ip_port) => {
                            self.apply_action(Action::PeerConnectionFailed { peer_addr: peer_ip_port });
                        },

                        TorrentCommand::ValidationComplete(pieces) => {
                            self.apply_action(Action::ValidationComplete { completed_pieces: pieces });
                        },

                        TorrentCommand::BlockSent { peer_id, bytes } => {
                            self.apply_action(Action::BlockSentToPeer {
                                peer_id,
                                byte_count: bytes
                            });
                        },
                        TorrentCommand::ValidationProgress(count) => {
                            self.apply_action(Action::ValidationProgress { count });
                        },

                        TorrentCommand::FatalStorageError(msg) => {
                            event!(Level::DEBUG, ?msg, "Fatal Storage error");
                            self.apply_action(Action::FatalError);
                        },

                        _ => {

                            println!("UNIMPLEMENTED TORRENT COMMEND {:?}",  command);
                        }
                    }
                }
            }
        }
    }
}
