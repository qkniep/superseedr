// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_file::Torrent;

use super::protocol::{
    reader_task, writer_task, BlockInfo, ClientExtendedId, ExtendedHandshakePayload, Message,
    MetadataMessage,
};

#[cfg(feature = "pex")]
use super::protocol::PexMessage;

use crate::token_bucket::TokenBucket;

use crate::command::TorrentCommand;

use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::Arc;

#[cfg(feature = "pex")]
use std::net::Ipv4Addr;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::split;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio::time::Duration;
use tokio::time::Instant;

use tracing::{event, instrument, Level};

use crate::torrent_manager::state::MAX_PIPELINE_DEPTH;

const PEER_BLOCK_IN_FLIGHT_LIMIT: usize = 8;
const MAX_WINDOW: usize = MAX_PIPELINE_DEPTH;

struct DisconnectGuard {
    peer_ip_port: String,
    manager_tx: Sender<TorrentCommand>,
}

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        let _ = self
            .manager_tx
            .try_send(TorrentCommand::Disconnect(self.peer_ip_port.clone()));
    }
}

struct AbortOnDrop(JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectionType {
    Outgoing,
    Incoming,
}

pub struct PeerSessionParameters {
    pub info_hash: Vec<u8>,
    pub torrent_metadata_length: Option<i64>,
    pub connection_type: ConnectionType,
    pub torrent_manager_rx: Receiver<TorrentCommand>,
    pub torrent_manager_tx: Sender<TorrentCommand>,
    pub peer_ip_port: String,
    pub client_id: Vec<u8>,
    pub global_dl_bucket: Arc<TokenBucket>,
    pub global_ul_bucket: Arc<TokenBucket>,
    pub shutdown_tx: broadcast::Sender<()>,
}

pub struct PeerSession {
    info_hash: Vec<u8>,
    peer_session_established: bool,
    torrent_metadata_length: Option<i64>,
    connection_type: ConnectionType,
    torrent_manager_rx: Receiver<TorrentCommand>,
    torrent_manager_tx: Sender<TorrentCommand>,
    client_id: Vec<u8>,
    peer_ip_port: String,

    writer_rx: Option<Receiver<Message>>,
    writer_tx: Sender<Message>,

    block_tracker: Arc<Mutex<HashSet<BlockInfo>>>,
    block_request_limit_semaphore: Arc<Semaphore>,

    peer_extended_id_mappings: HashMap<String, u8>,
    peer_extended_handshake_payload: Option<ExtendedHandshakePayload>,
    peer_torrent_metadata_piece_count: usize,
    peer_torrent_metadata_pieces: Vec<u8>,

    global_dl_bucket: Arc<TokenBucket>,
    global_ul_bucket: Arc<TokenBucket>,

    shutdown_tx: broadcast::Sender<()>,

    current_window_size: usize,
    blocks_received_interval: usize,
    prev_speed: f64,
    pending_window_shrink: usize,
    peer_upload_queue_len: usize,
    last_piece_received: Instant,

    #[cfg(test)]
    testing_window_monitor: Option<Arc<AtomicUsize>>,
}

impl PeerSession {
    pub fn new(params: PeerSessionParameters) -> Self {
        // Increased channel size to prevent internal bottlenecks
        let (writer_tx, writer_rx) = mpsc::channel::<Message>(1000);

        Self {
            info_hash: params.info_hash,
            peer_session_established: false,
            torrent_metadata_length: params.torrent_metadata_length,
            connection_type: params.connection_type,
            torrent_manager_rx: params.torrent_manager_rx,
            torrent_manager_tx: params.torrent_manager_tx,
            client_id: params.client_id,
            peer_ip_port: params.peer_ip_port,
            writer_rx: Some(writer_rx),
            writer_tx,
            block_tracker: Arc::new(Mutex::new(HashSet::new())),
            block_request_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),

            peer_extended_id_mappings: HashMap::new(),
            peer_extended_handshake_payload: None,
            peer_torrent_metadata_piece_count: 0,
            peer_torrent_metadata_pieces: Vec::new(),
            global_dl_bucket: params.global_dl_bucket,
            global_ul_bucket: params.global_ul_bucket,
            shutdown_tx: params.shutdown_tx,

            current_window_size: PEER_BLOCK_IN_FLIGHT_LIMIT,
            blocks_received_interval: 0,
            prev_speed: 0.0,
            pending_window_shrink: 0,
            peer_upload_queue_len: 0,
            last_piece_received: Instant::now(),

            #[cfg(test)]
            testing_window_monitor: None,
        }
    }

    #[instrument(skip(self, stream, current_bitfield))]
    pub async fn run<S>(
        mut self,
        stream: S,
        handshake_response: Vec<u8>,
        current_bitfield: Option<Vec<u8>>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let _guard = DisconnectGuard {
            peer_ip_port: self.peer_ip_port.clone(),
            manager_tx: self.torrent_manager_tx.clone(),
        };

        // 1. Split Stream
        let (mut stream_read_half, stream_write_half) = split(stream);
        let (error_tx, mut error_rx) = oneshot::channel();

        // 2. Spawn Writer Task
        let global_ul_bucket_clone = self.global_ul_bucket.clone();
        let writer_shutdown_rx = self.shutdown_tx.subscribe();
        let writer_rx = self.writer_rx.take().ok_or("Writer RX missing")?;

        let writer_handle = tokio::spawn(writer_task(
            stream_write_half,
            writer_rx,
            error_tx,
            global_ul_bucket_clone,
            writer_shutdown_rx,
        ));
        let _writer_abort_guard = AbortOnDrop(writer_handle);

        // 3. Perform Handshake (Synchronous Phase)
        // We do this BEFORE spawning the reader task so we can validate the connection.
        let handshake_response = match self.connection_type {
            ConnectionType::Outgoing => {
                let _ = self.writer_tx.try_send(Message::Handshake(
                    self.info_hash.clone(),
                    self.client_id.clone(),
                ));
                let mut buffer = vec![0u8; 68];
                stream_read_half.read_exact(&mut buffer).await?;
                buffer
            }
            ConnectionType::Incoming => {
                let _ = self.writer_tx.try_send(Message::Handshake(
                    self.info_hash.clone(),
                    self.client_id.clone(),
                ));
                handshake_response
            }
        };

        let peer_info_hash = &handshake_response[28..48];
        if self.info_hash != peer_info_hash {
            return Err("Info hash mismatch".into());
        }

        let peer_id = handshake_response[48..68].to_vec();
        let _ = self
            .torrent_manager_tx
            .try_send(TorrentCommand::PeerId(self.peer_ip_port.clone(), peer_id));

        if (handshake_response[25] & 0x10) != 0 {
            let meta_len = self.torrent_metadata_length;
            let _ = self
                .writer_tx
                .try_send(Message::ExtendedHandshake(meta_len));
        }

        if let Some(bitfield) = current_bitfield {
            self.peer_session_established = true;
            let _ = self.writer_tx.try_send(Message::Bitfield(bitfield));
            let _ = self
                .torrent_manager_tx
                .try_send(TorrentCommand::SuccessfullyConnected(
                    self.peer_ip_port.clone(),
                ));
        }

        let (peer_msg_tx, mut peer_msg_rx) = mpsc::channel::<Message>(100);
        let reader_shutdown = self.shutdown_tx.subscribe();
        let dl_bucket = self.global_dl_bucket.clone();
        let reader_handle = tokio::spawn(reader_task(
            stream_read_half,
            peer_msg_tx,
            dl_bucket,
            reader_shutdown,
        ));
        let _reader_abort_guard = AbortOnDrop(reader_handle);

        // 5. Main Event Loop
        let mut keep_alive_timer = tokio::time::interval(Duration::from_secs(60));
        let inactivity_timeout = tokio::time::sleep(Duration::from_secs(120));
        tokio::pin!(inactivity_timeout);

        let mut speed_adjustment_timer = tokio::time::interval(Duration::from_secs(1));
        speed_adjustment_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let manager_tx = self.torrent_manager_tx.clone();

        let _result: Result<(), Box<dyn StdError + Send + Sync>> = 'session: loop {
            tokio::select! {
                // Timeout Check
                _ = &mut inactivity_timeout => break 'session Err("Timeout".into()),

                // KeepAlive
                _ = keep_alive_timer.tick() => { let _ = self.writer_tx.try_send(Message::KeepAlive); },

                _ = speed_adjustment_timer.tick() => { self.adjust_window_size(); },

                // INCOMING MESSAGES (From Reader Task)
                Some(msg) = peer_msg_rx.recv() => {
                    inactivity_timeout.as_mut().reset(Instant::now() + Duration::from_secs(120));

                    match msg {
                        Message::Piece(index, begin, data) => {
                            let block_len = data.len() as u32;
                            let info = BlockInfo {
                                piece_index: index,
                                offset: begin,
                                length: block_len,
                            };

                            let was_expected = self.block_tracker.lock().await.remove(&info);

                            if was_expected {
                                self.blocks_received_interval += 1;
                                self.last_piece_received = Instant::now();

                                if self.pending_window_shrink > 0 {
                                    self.pending_window_shrink -= 1;
                                    // We do NOT call add_permits(1).
                                    // This effectively destroys the permit associated with this block,
                                    // realizing the window shrinkage.
                                } else {
                                    self.block_request_limit_semaphore.add_permits(1);
                                }

                                let cmd = TorrentCommand::Block(self.peer_ip_port.clone(), index, begin, data);

                                loop {
                                    tokio::select! {
                                        permit_res = manager_tx.reserve() => {
                                            match permit_res {
                                                Ok(permit) => {
                                                    permit.send(cmd);
                                                    break;
                                                }
                                                Err(_) => break 'session Err("Manager Closed".into()),
                                            }
                                        }
                                        // Still process Manager commands while waiting to send (Avoid Deadlock)
                                        Some(cmd) = self.torrent_manager_rx.recv() => {
                                            if !self.process_manager_command(cmd)? {
                                                break 'session Ok(());
                                            }
                                        },
                                        _ = shutdown_rx.recv() => break 'session Ok(()),
                                    }
                                }
                            } else {
                                event!(Level::TRACE, "Session: Dropped cancelled/unsolicited block {}@{}", index, begin);
                            }
                        }
                        Message::Choke => {
                            self.block_tracker.lock().await.clear();

                            self.pending_window_shrink = 0;

                            self.current_window_size = PEER_BLOCK_IN_FLIGHT_LIMIT;

                            #[cfg(test)]
                            if let Some(monitor) = &self.testing_window_monitor {
                                monitor.store(self.current_window_size, Ordering::Relaxed);
                            }

                            let current = self.block_request_limit_semaphore.available_permits();
                            if current < self.current_window_size {
                                self.block_request_limit_semaphore.add_permits(self.current_window_size - current);
                            }

                            let _ = self.torrent_manager_tx.try_send(TorrentCommand::Choke(self.peer_ip_port.clone()));
                        }
                        Message::Unchoke => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::Unchoke(self.peer_ip_port.clone())); }
                        Message::Interested => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::PeerInterested(self.peer_ip_port.clone())); }
                        Message::NotInterested => {}
                        Message::Have(idx) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::Have(self.peer_ip_port.clone(), idx)); }
                        Message::Bitfield(bf) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::PeerBitfield(self.peer_ip_port.clone(), bf)); }
                        Message::Request(i, b, l) => {
                            if self.peer_upload_queue_len < MAX_WINDOW {
                                self.peer_upload_queue_len += 1;
                                let _ = self.torrent_manager_tx.try_send(
                                    TorrentCommand::RequestUpload(self.peer_ip_port.clone(), i, b, l)
                                );
                            } else {
                                // Drop the request if they are flooding us
                                tracing::warn!("Peer {} upload queue full. Dropping request.", self.peer_ip_port);
                            }
                        }

                        Message::Cancel(i, b, l) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::CancelUpload(self.peer_ip_port.clone(), i, b, l)); }
                        Message::Extended(id, p) => { self.handle_extended_message(id, p).await?; }
                        Message::KeepAlive => {}
                        Message::Port(_) => {}
                        Message::Handshake(..) => {}
                        Message::ExtendedHandshake(_) => {}

                        Message::HashRequest(root_idx, base, offset, length) => {
                            let _ = self.torrent_manager_tx.try_send(TorrentCommand::GetHashes {
                                peer_id: self.peer_ip_port.clone(),
                                file_root: vec![], // Manager resolves this
                                base_layer: base,
                                index: offset,     // 'offset' is the start index they want
                                length,
                                proof_layers: 0,   // Not provided in basic request
                            });
                            // We log the root_idx so it's not unused
                            tracing::trace!("Peer requested hashes for RootID: {}", root_idx);
                        }

                        Message::HashPiece(root_idx, base, offset, proof) => {
                            let _ = self.torrent_manager_tx.try_send(
                                TorrentCommand::MerkleHashData {
                                    peer_id: self.peer_ip_port.clone(),
                                    file_index: root_idx,
                                    piece_index: offset,
                                    base_layer: base,
                                    length: proof.len() as u32 / 32,
                                    proof,
                                }
                            );
                            tracing::trace!("Received HashPiece for RootID: {}", root_idx);
                        }

                        Message::HashReject(root_idx, _, offset, _) => {
                            tracing::debug!("Peer {} rejected hash request for RootID {} @ Offset {}", self.peer_ip_port, root_idx, offset);
                        }
                    }
                },

                // OUTGOING COMMANDS (From Manager)
                Some(cmd) = self.torrent_manager_rx.recv() => {
                    if !self.process_manager_command(cmd)? { break 'session Ok(()); }
                },

                // WRITER ERRORS
                writer_res = &mut error_rx => {
                    break 'session Err(writer_res.unwrap_or_else(|_| "Writer panicked".into()));
                },

                // SHUTDOWN
                msg = shutdown_rx.recv() => {
                    match msg {
                        Ok(()) => break 'session Ok(()),
                        Err(_) => continue,
                    }
                },
            }
        };

        Ok(())
    }

    fn process_manager_command(
        &mut self,
        command: TorrentCommand,
    ) -> Result<bool, Box<dyn StdError + Send + Sync>> {
        match command {
            TorrentCommand::Disconnect(_) => return Ok(false),

            TorrentCommand::PeerChoke | TorrentCommand::Choke(_) => {
                let _ = self.writer_tx.try_send(Message::Choke);
            }
            TorrentCommand::PeerUnchoke | TorrentCommand::Unchoke(_) => {
                let _ = self.writer_tx.try_send(Message::Unchoke);
            }
            TorrentCommand::ClientInterested => {
                let _ = self.writer_tx.try_send(Message::Interested);
            }
            TorrentCommand::NotInterested => {
                let _ = self.writer_tx.try_send(Message::NotInterested);
            }

            // --- BULK REQUEST WITH ZOMBIE REAPER ---
            TorrentCommand::BulkRequest(requests) => {
                let writer = self.writer_tx.clone();
                let sem = self.block_request_limit_semaphore.clone();
                let tracker = self.block_tracker.clone();
                let mut shutdown = self.shutdown_tx.subscribe();

                tokio::spawn(async move {
                    for (index, begin, length) in requests {
                        let permit_option = tokio::select! {
                            permit_result = timeout(Duration::from_secs(10), sem.clone().acquire_owned()) => {
                                match permit_result {
                                    Ok(Ok(permit)) => Some(permit),
                                    _ => None,
                                }
                            }
                            _ = shutdown.recv() => None
                        };

                        if let Some(permit) = permit_option {
                            let info = BlockInfo {
                                piece_index: index,
                                offset: begin,
                                length,
                            };

                            {
                                let mut t = tracker.lock().await;
                                t.insert(info.clone());
                            }

                            if writer
                                .send(Message::Request(index, begin, length))
                                .await
                                .is_ok()
                            {
                                permit.forget();
                            } else {
                                {
                                    let mut t = tracker.lock().await;
                                    t.remove(&info);
                                }
                                break;
                            }
                        }
                    }
                });
            }

            TorrentCommand::BulkCancel(cancels) => {
                for (index, begin, len) in &cancels {
                    let _ = self
                        .writer_tx
                        .try_send(Message::Cancel(*index, *begin, *len));
                }

                let tracker = self.block_tracker.clone();
                let sem = self.block_request_limit_semaphore.clone();

                tokio::spawn(async move {
                    let mut tracker_guard = tracker.lock().await;
                    let mut permits_to_add = 0;
                    for (index, begin, length) in cancels {
                        let info = BlockInfo {
                            piece_index: index,
                            offset: begin,
                            length,
                        };
                        if tracker_guard.remove(&info) {
                            permits_to_add += 1;
                        }
                    }
                    if permits_to_add > 0 {
                        sem.add_permits(permits_to_add);
                    }
                });
            }

            TorrentCommand::Upload(index, begin, data) => {
                if self.peer_upload_queue_len > 0 {
                    self.peer_upload_queue_len -= 1;
                }
                let _ = self.writer_tx.try_send(Message::Piece(index, begin, data));
            }
            TorrentCommand::PeerBitfield(_, bf) => {
                let _ = self.writer_tx.try_send(Message::Bitfield(bf));
            }
            #[cfg(feature = "pex")]
            TorrentCommand::SendPexPeers(peers) => {
                self.handle_pex(peers);
            }
            TorrentCommand::Have(_, idx) => {
                let _ = self.writer_tx.try_send(Message::Have(idx));
            }
            TorrentCommand::SendHashPiece {
                root: _,
                base_layer,
                index,
                proof,
                ..
            } => {
                // Protocol expects: HashPiece(u32, u32, u32, Vec<u8>)
                // We map them to: index, base, offset, proof
                let _ = self
                    .writer_tx
                    .try_send(Message::HashPiece(index, base_layer, 0, proof));
            }

            // [CORRECTED] Removed 'root' from Message::HashReject construction
            TorrentCommand::SendHashReject {
                root: _,
                base_layer,
                index,
                length,
                ..
            } => {
                // Protocol expects: HashReject(u32, u32, u32, u32)
                let _ = self
                    .writer_tx
                    .try_send(Message::HashReject(index, base_layer, 0, length));
            }
            _ => {}
        }
        Ok(true)
    }

    #[cfg(feature = "pex")]
    fn handle_pex(&self, peers_list: Vec<String>) {
        if let Some(pex_id) = self
            .peer_extended_id_mappings
            .get(ClientExtendedId::UtPex.as_str())
            .copied()
        {
            let added: Vec<u8> = peers_list
                .iter()
                .filter(|&ip| *ip != self.peer_ip_port)
                .filter_map(|ip| ip.parse::<std::net::SocketAddr>().ok())
                .flat_map(|addr| match addr {
                    std::net::SocketAddr::V4(v4) => {
                        let mut b = v4.ip().octets().to_vec();
                        b.extend_from_slice(&v4.port().to_be_bytes());
                        Some(b)
                    }
                    _ => None,
                })
                .flatten()
                .collect();

            if !added.is_empty() {
                let msg = PexMessage {
                    added,
                    ..Default::default()
                };
                if let Ok(payload) = serde_bencode::to_bytes(&msg) {
                    let _ = self.writer_tx.try_send(Message::Extended(pex_id, payload));
                }
            }
        }
    }

    async fn handle_extended_message(
        &mut self,
        extended_id: u8,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if extended_id == ClientExtendedId::Handshake.id() {
            if let Ok(handshake_data) =
                serde_bencode::from_bytes::<ExtendedHandshakePayload>(&payload)
            {
                self.peer_extended_id_mappings = handshake_data.m.clone();
                if !handshake_data.m.is_empty() {
                    self.peer_extended_handshake_payload = Some(handshake_data.clone());
                    if !self.peer_session_established {
                        if let Some(_torrent_metadata_len) = handshake_data.metadata_size {
                            let request = MetadataMessage {
                                msg_type: 0,
                                piece: 0,
                                total_size: None,
                            };
                            if let Ok(payload_bytes) = serde_bencode::to_bytes(&request) {
                                let _ = self.writer_tx.try_send(Message::Extended(
                                    ClientExtendedId::UtMetadata.id(),
                                    payload_bytes,
                                ));
                            }
                        }
                    }
                }
            }
        }

        #[cfg(feature = "pex")]
        {
            if extended_id == ClientExtendedId::UtPex.id() {
                if let Ok(pex_data) = serde_bencode::from_bytes::<PexMessage>(&payload) {
                    let mut new_peers = Vec::new();
                    for chunk in pex_data.added.chunks_exact(6) {
                        let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
                        let port = u16::from_be_bytes([chunk[4], chunk[5]]);
                        new_peers.push((ip.to_string(), port));
                    }
                    if !new_peers.is_empty() {
                        let _ = self
                            .torrent_manager_tx
                            .try_send(TorrentCommand::AddPexPeers(
                                self.peer_ip_port.clone(),
                                new_peers,
                            ));
                    }
                }
            }
        }

        if extended_id == ClientExtendedId::UtMetadata.id() && !self.peer_session_established {
            if let Some(ref handshake_data) = self.peer_extended_handshake_payload {
                if let Some(torrent_metadata_len) = handshake_data.metadata_size {
                    let torrent_metadata_len_usize = torrent_metadata_len as usize;
                    let current_offset = self.peer_torrent_metadata_piece_count * 16384;
                    let expected_data_len = std::cmp::min(
                        16384,
                        torrent_metadata_len_usize.saturating_sub(current_offset),
                    );

                    if payload.len() >= expected_data_len {
                        let header_len = payload.len() - expected_data_len;
                        let metadata_binary = &payload[header_len..];
                        self.peer_torrent_metadata_pieces.extend(metadata_binary);

                        if torrent_metadata_len_usize == self.peer_torrent_metadata_pieces.len() {
                            if let Ok(dht_info) =
                                serde_bencode::from_bytes(&self.peer_torrent_metadata_pieces[..])
                            {
                                let _ =
                                    self.torrent_manager_tx.try_send(TorrentCommand::DhtTorrent(
                                        Box::new(Torrent {
                                            info_dict_bencode: self
                                                .peer_torrent_metadata_pieces
                                                .clone(),
                                            info: dht_info,
                                            announce: None,
                                            announce_list: None,
                                            url_list: None,
                                            creation_date: None,
                                            comment: None,
                                            created_by: None,
                                            encoding: None,
                                            piece_layers: None,
                                        }),
                                        torrent_metadata_len,
                                    ));
                            }
                        } else {
                            self.peer_torrent_metadata_piece_count += 1;
                            let request = MetadataMessage {
                                msg_type: 0,
                                piece: self.peer_torrent_metadata_piece_count,
                                total_size: None,
                            };
                            if let Ok(payload_bytes) = serde_bencode::to_bytes(&request) {
                                let _ = self.writer_tx.try_send(Message::Extended(
                                    ClientExtendedId::UtMetadata.id(),
                                    payload_bytes,
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn adjust_window_size(&mut self) {
        let available_permits = self.block_request_limit_semaphore.available_permits();
        let in_flight = self.current_window_size.saturating_sub(available_permits);

        if in_flight > 0 && self.last_piece_received.elapsed() > Duration::from_secs(20) {
            tracing::error!(
                "Peer {} stalled ({} blocks in flight, no data for 20s). Disconnecting.",
                self.peer_ip_port,
                in_flight
            );
            let _ = self
                .torrent_manager_tx
                .try_send(TorrentCommand::Disconnect(self.peer_ip_port.clone()));
            return;
        }

        let speed = self.blocks_received_interval as f64;
        self.blocks_received_interval = 0; // Reset counter for the next 1s tick

        let is_saturated = available_permits <= 2;
        if is_saturated {
            if speed > self.prev_speed * 1.1 {
                if self.current_window_size < MAX_WINDOW {
                    self.current_window_size += 1;
                    self.block_request_limit_semaphore.add_permits(1);

                    tracing::debug!(
                        "Speed Up: Peer {} -> {:.2} blocks/s (was {:.2}). Window: {}",
                        self.peer_ip_port,
                        speed,
                        self.prev_speed,
                        self.current_window_size
                    );
                }
            } else if speed < self.prev_speed * 0.9 {
                self.shrink_window();
            }
        } else if available_permits > (self.current_window_size / 2) {
            self.shrink_window();
        }

        #[cfg(test)]
        if let Some(monitor) = &self.testing_window_monitor {
            monitor.store(self.current_window_size, Ordering::Relaxed);
        }

        if self.prev_speed == 0.0 || speed > 0.0 {
            self.prev_speed = speed;
        }
    }

    fn shrink_window(&mut self) {
        if self.current_window_size > PEER_BLOCK_IN_FLIGHT_LIMIT {
            self.current_window_size -= 1;
            if let Ok(permit) = self.block_request_limit_semaphore.try_acquire() {
                permit.forget();
            } else {
                self.pending_window_shrink += 1;
            }

            tracing::debug!(
                "Shrinking: Peer {} Limit reduced to {}",
                self.peer_ip_port,
                self.current_window_size
            );
        }
    }

    #[cfg(test)]
    pub fn with_window_monitor(mut self, monitor: Arc<AtomicUsize>) -> Self {
        self.testing_window_monitor = Some(monitor);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::protocol::{generate_message, Message};

    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{broadcast, mpsc};

    async fn parse_message<R>(stream: &mut R) -> Result<Message, std::io::Error>
    where
        R: AsyncReadExt + Unpin,
    {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let message_len = u32::from_be_bytes(len_buf);

        let mut message_buf = if message_len > 0 {
            let payload_len = message_len as usize;
            let mut temp_buf = vec![0; payload_len];
            stream.read_exact(&mut temp_buf).await?;
            temp_buf
        } else {
            vec![]
        };

        let mut full_message = len_buf.to_vec();
        full_message.append(&mut message_buf);
        let mut cursor = std::io::Cursor::new(&full_message);
        crate::networking::protocol::parse_message_from_bytes(&mut cursor)
    }

    // --- Helper: Spawn Session with Window Monitor ---
    async fn spawn_test_session() -> (
        tokio::io::DuplexStream,        // Network (Mock Peer)
        mpsc::Sender<TorrentCommand>,   // Client Command Tx
        mpsc::Receiver<TorrentCommand>, // Manager Event Rx
        Arc<AtomicUsize>,               // <--- The Window Monitor
    ) {
        let (client_socket, mock_peer_socket) = duplex(64 * 1024 * 1024);
        let infinite_bucket = Arc::new(TokenBucket::new(f64::INFINITY, f64::INFINITY));
        let (manager_tx, manager_rx) = mpsc::channel(1000);
        let (cmd_tx, cmd_rx) = mpsc::channel(1000);
        let (shutdown_tx, _) = broadcast::channel(1);

        let params = PeerSessionParameters {
            info_hash: [0u8; 20].to_vec(),
            torrent_metadata_length: None,
            connection_type: ConnectionType::Outgoing,
            torrent_manager_rx: cmd_rx,
            torrent_manager_tx: manager_tx,
            peer_ip_port: "virtual-peer:1337".to_string(),
            client_id: b"-SS1000-TESTTESTTEST".to_vec(),
            global_dl_bucket: infinite_bucket.clone(),
            global_ul_bucket: infinite_bucket.clone(),
            shutdown_tx,
        };

        // Create the Atomic Monitor
        let window_monitor = Arc::new(AtomicUsize::new(PEER_BLOCK_IN_FLIGHT_LIMIT));
        let monitor_clone = window_monitor.clone();

        tokio::spawn(async move {
            // Inject monitor using the builder pattern
            let session = PeerSession::new(params).with_window_monitor(monitor_clone);

            if let Err(e) = session.run(client_socket, vec![], Some(vec![])).await {
                eprintln!("Test Session ended: {:?}", e);
            }
        });

        (mock_peer_socket, cmd_tx, manager_rx, window_monitor)
    }

    // --- Standard Handshake Helper ---
    async fn perform_handshake(network: &mut tokio::io::DuplexStream) {
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.unwrap();
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.unwrap();
    }

    #[tokio::test]
    async fn test_pipeline_saturation_with_virtual_time() {
        let (mut network, client_cmd_tx, _manager_event_rx, _) = spawn_test_session().await;

        // --- Step 1: Handshake ---
        let mut handshake_buf = vec![0u8; 68];
        network
            .read_exact(&mut handshake_buf)
            .await
            .expect("Failed to read client handshake");

        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network
            .write_all(&response)
            .await
            .expect("Failed to write handshake");

        // Consume Initial Messages (Bitfield, Extended Handshake, etc.)
        // We read until we stop getting messages for a short duration
        let start_drain = Instant::now();
        while start_drain.elapsed() < Duration::from_millis(500) {
            if let Ok(Ok(_)) = timeout(Duration::from_millis(50), parse_message(&mut network)).await
            {
                continue;
            } else {
                break; // No more immediate messages
            }
        }

        // --- Step 2: The Saturation Test ---
        // Send 5 requests in a single bulk command.
        let requests: Vec<_> = (0..5).map(|i| (0, i * 16384, 16384)).collect();
        client_cmd_tx
            .send(TorrentCommand::BulkRequest(requests))
            .await
            .expect("Failed to send bulk command");

        // ASSERTION: Immediate Burst
        let mut requests_received = HashSet::new();

        // Give 5 seconds for all async tasks to spawn and flush
        let overall_timeout = Duration::from_secs(5);
        let start = Instant::now();

        while requests_received.len() < 5 {
            if start.elapsed() > overall_timeout {
                break; // Stop loop, assert later
            }

            // Per-message timeout
            match timeout(Duration::from_secs(1), parse_message(&mut network)).await {
                Ok(Ok(Message::Request(idx, begin, len))) => {
                    assert_eq!(idx, 0);
                    assert_eq!(len, 16384);
                    requests_received.insert(begin);
                }
                Ok(Ok(_)) => {}      // Ignore KeepAlives or late Metadata messages
                Ok(Err(_)) => break, // Socket closed
                Err(_) => {}         // Timeout, keep retrying until overall_timeout
            }
        }

        assert_eq!(
            requests_received.len(),
            5,
            "Failed to receive all 5 requests in burst. Got: {:?}",
            requests_received
        );
    }

    #[tokio::test]
    async fn test_fragmented_pipeline_saturation() {
        let (mut network, client_cmd_tx, _manager_event_rx, _) = spawn_test_session().await;

        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.unwrap();
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.unwrap();

        // Drain setup
        let start_drain = Instant::now();
        while start_drain.elapsed() < Duration::from_millis(500) {
            if let Ok(Ok(_)) = timeout(Duration::from_millis(50), parse_message(&mut network)).await
            {
                continue;
            } else {
                break;
            }
        }

        // Send 5 separate commands for 5 separate pieces in a single bulk command
        let requests: Vec<_> = (0..5).map(|i| (i as u32, 0, 16384)).collect();
        client_cmd_tx
            .send(TorrentCommand::BulkRequest(requests))
            .await
            .expect("Failed to send bulk command");

        let mut requested_pieces = HashSet::new();
        let start = Instant::now();

        while requested_pieces.len() < 5 {
            if start.elapsed() > Duration::from_secs(5) {
                break;
            }

            if let Ok(Ok(Message::Request(idx, _, _))) =
                timeout(Duration::from_secs(1), parse_message(&mut network)).await
            {
                requested_pieces.insert(idx);
            }
        }

        assert_eq!(
            requested_pieces.len(),
            5,
            "Failed to receive all 5 fragmented requests. Got: {:?}",
            requested_pieces
        );
    }

    #[tokio::test]
    async fn test_performance_1000_blocks_sliding_window() {
        // 1. Setup Session
        let (mut network, client_cmd_tx, mut manager_event_rx, _) = spawn_test_session().await;

        // 2. Handshake
        let mut handshake_buf = vec![0u8; 68];
        network
            .read_exact(&mut handshake_buf)
            .await
            .expect("Handshake read failed");

        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network
            .write_all(&response)
            .await
            .expect("Handshake write failed");

        // 3. Spawn the "Smart Peer"
        let (mut peer_read, mut peer_write) = tokio::io::split(network);

        tokio::spawn(async move {
            let mut am_choking = true;

            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(5), parse_message(&mut peer_read)).await
            {
                match msg {
                    Message::Interested => {
                        if am_choking {
                            let unchoke = generate_message(Message::Unchoke).unwrap();
                            peer_write.write_all(&unchoke).await.unwrap();
                            am_choking = false;
                        }
                    }
                    Message::Request(index, begin, _len) => {
                        if !am_choking {
                            let data = vec![1u8; 16384];
                            let piece =
                                generate_message(Message::Piece(index, begin, data)).unwrap();
                            if peer_write.write_all(&piece).await.is_err() {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // 4. Manager Startup
        let mut session_ready = false;
        while !session_ready {
            match timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::SuccessfullyConnected(_))) => session_ready = true,
                Ok(Some(TorrentCommand::PeerBitfield(_, _))) => session_ready = true,
                Ok(Some(_)) => continue,
                _ => panic!("Session failed to connect"),
            }
        }

        client_cmd_tx
            .send(TorrentCommand::ClientInterested)
            .await
            .unwrap();

        let mut is_unchoked = false;
        while !is_unchoked {
            if let Ok(Some(cmd)) = timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                if let TorrentCommand::Unchoke(_) = cmd {
                    is_unchoked = true;
                }
            } else {
                panic!("Peer never unchoked us!");
            }
        }

        // 5. Sliding Window Test
        const TOTAL_BLOCKS: u32 = 1000;
        const WINDOW_SIZE: u32 = 20;
        const BLOCK_SIZE: usize = 16384;

        let start_time = Instant::now();
        let mut blocks_requested = 0;
        let mut blocks_received = 0;

        // Fill window
        let requests: Vec<_> = (0..WINDOW_SIZE)
            .map(|i| (i, 0, BLOCK_SIZE as u32))
            .collect();
        client_cmd_tx
            .send(TorrentCommand::BulkRequest(requests))
            .await
            .unwrap();
        blocks_requested += WINDOW_SIZE;

        // Process loop
        while blocks_received < TOTAL_BLOCKS {
            match timeout(Duration::from_secs(5), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Block(..))) => {
                    blocks_received += 1;
                    if blocks_requested < TOTAL_BLOCKS {
                        client_cmd_tx
                            .send(TorrentCommand::BulkRequest(vec![(
                                blocks_requested,
                                0,
                                BLOCK_SIZE as u32,
                            )]))
                            .await
                            .unwrap();
                        blocks_requested += 1;
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Session died"),
                Err(_) => panic!("Stalled at {}/{}", blocks_received, TOTAL_BLOCKS),
            }
        }

        let elapsed = start_time.elapsed();
        let total_mb = (TOTAL_BLOCKS * BLOCK_SIZE as u32) as f64 / 1_000_000.0;
        println!(
            "Success: {:.2} MB in {:.2?} ({:.2} MB/s)",
            total_mb,
            elapsed,
            total_mb / elapsed.as_secs_f64()
        );
    }

    #[tokio::test]
    async fn test_bug_repro_unsolicited_forwarding() {
        let (mut network, _client_cmd_tx, mut manager_rx, _) = spawn_test_session().await;

        // 1. Handshake
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.unwrap();
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.unwrap();

        // Drain setup messages on the network side
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            if let Ok(Ok(_)) = timeout(Duration::from_millis(10), parse_message(&mut network)).await
            {
                continue;
            } else {
                break;
            }
        }

        // 2. THE TRIGGER: Send a Block we NEVER requested
        // Piece 999 is definitely not in the session's tracker.
        let data = vec![0xAA; 16384];
        let piece_msg = generate_message(Message::Piece(999, 0, data)).unwrap();
        network.write_all(&piece_msg).await.unwrap();

        // 3. ASSERTION
        // We listen to the Manager channel for a fixed window.
        // We MUST loop because the Session sends 'PeerId', 'SuccessfullyConnected', etc.
        // first. If we only recv() once, we pop 'PeerId', ignore it, and exit early
        // (passing the test falsely).

        let listen_duration = Duration::from_millis(500);
        let start_listen = Instant::now();

        while start_listen.elapsed() < listen_duration {
            // Short timeout per recv to allow checking the total elapsed time
            match timeout(Duration::from_millis(50), manager_rx.recv()).await {
                Ok(Some(TorrentCommand::Block(peer_id, index, begin, _))) => {
                    panic!(
                        "TEST FAILED (BUG CONFIRMED): Session forwarded unsolicited block {}@{} from {}! \
                        It should have been dropped because it was not in the tracker.", 
                        index, begin, peer_id
                    );
                }
                Ok(Some(_cmd)) => {
                    // Continue loop, draining unrelated startup events (PeerId, Bitfield, etc.)
                    continue;
                }
                Ok(None) => panic!("Session died unexpectedly"),
                Err(_) => continue, // Timeout on individual recv, keep listening until total time is up
            }
        }

        println!("SUCCESS: Session filtered out the unsolicited block.");
    }

    async fn spawn_debug_session() -> (
        tokio::io::DuplexStream,
        mpsc::Sender<TorrentCommand>,
        mpsc::Receiver<TorrentCommand>,
        tokio::task::JoinHandle<()>, // <--- Return the handle
    ) {
        // Use a large buffer to prevent blocking
        let (client_socket, mock_peer_socket) = duplex(64 * 1024 * 1024);
        let infinite_bucket = Arc::new(TokenBucket::new(f64::INFINITY, f64::INFINITY));
        let (manager_tx, manager_rx) = mpsc::channel(1000);
        let (cmd_tx, cmd_rx) = mpsc::channel(1000);
        let (shutdown_tx, _) = broadcast::channel(1);

        let params = PeerSessionParameters {
            info_hash: [0u8; 20].to_vec(),
            torrent_metadata_length: None,
            connection_type: ConnectionType::Outgoing,
            torrent_manager_rx: cmd_rx,
            torrent_manager_tx: manager_tx,
            peer_ip_port: "virtual-peer:1337".to_string(),
            client_id: b"-SS1000-TESTTESTTEST".to_vec(),
            global_dl_bucket: infinite_bucket.clone(),
            global_ul_bucket: infinite_bucket.clone(),
            shutdown_tx,
        };

        let handle = tokio::spawn(async move {
            let session = PeerSession::new(params);
            match session.run(client_socket, vec![], Some(vec![])).await {
                Ok(_) => println!("DEBUG: Session exited cleanly"),
                Err(e) => {
                    // This print is CRITICAL for seeing why it died
                    println!("DEBUG: Session CRASHED with error: {:?}", e);
                    // Force a panic here so the JoinHandle reports it as a panic to the test
                    panic!("Session crashed: {:?}", e);
                }
            }
        });

        (mock_peer_socket, cmd_tx, manager_rx, handle)
    }

    #[tokio::test]
    async fn test_heavy_load_20k_blocks_sliding_window() {
        const TOTAL_BLOCKS: u32 = 20_000;
        const PIPELINE_DEPTH: u32 = 128;
        const BLOCK_SIZE: usize = 16384;

        // 1. Setup Session using the DEBUG helper
        let (mut network, client_cmd_tx, mut manager_event_rx, session_handle) =
            spawn_debug_session().await;

        // 2. Handshake
        let mut handshake_buf = vec![0u8; 68];
        network
            .read_exact(&mut handshake_buf)
            .await
            .expect("Handshake read failed");
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network
            .write_all(&response)
            .await
            .expect("Handshake write failed");

        // 3. Mock Peer (High Perf)
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let mut am_choking = true;
            let dummy_data = vec![0xAA; BLOCK_SIZE];
            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await
            {
                match msg {
                    Message::Interested => {
                        if am_choking {
                            let unchoke = generate_message(Message::Unchoke).unwrap();
                            if peer_write.write_all(&unchoke).await.is_err() {
                                break;
                            }
                            am_choking = false;
                        }
                    }
                    Message::Request(index, begin, _len) => {
                        if !am_choking {
                            let piece_msg =
                                generate_message(Message::Piece(index, begin, dummy_data.clone()))
                                    .unwrap();
                            if peer_write.write_all(&piece_msg).await.is_err() {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // 4. Wait for Ready
        // We add a check for the session handle here too, in case it dies during startup
        loop {
            tokio::select! {
                res = manager_event_rx.recv() => match res {
                    Some(TorrentCommand::SuccessfullyConnected(_)) => break,
                    Some(TorrentCommand::PeerBitfield(..)) => break,
                    Some(_) => continue,
                    None => {
                        println!("Session died during startup. checking handle...");
                        let _ = session_handle.await;
                        panic!("Session died during startup (Manager RX Closed)");
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(2)) => panic!("Timeout waiting for connect"),
            }
        }

        client_cmd_tx
            .send(TorrentCommand::ClientInterested)
            .await
            .unwrap();

        // Wait for Unchoke
        loop {
            tokio::select! {
                res = manager_event_rx.recv() => match res {
                    Some(TorrentCommand::Unchoke(_)) => break,
                    Some(_) => continue,
                    None => {
                        let _ = session_handle.await;
                        panic!("Session died waiting for Unchoke");
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(2)) => panic!("Timeout waiting for Unchoke"),
            }
        }

        // 5. Stress Test
        println!("Starting transfer of {} blocks...", TOTAL_BLOCKS);
        tokio::task::yield_now().await;

        let start_time = Instant::now();
        let mut blocks_requested = 0;
        let mut blocks_received = 0;

        let initial_batch: Vec<_> = (0..PIPELINE_DEPTH)
            .map(|i| {
                blocks_requested += 1;
                (i, 0, BLOCK_SIZE as u32)
            })
            .collect();

        client_cmd_tx
            .send(TorrentCommand::BulkRequest(initial_batch))
            .await
            .expect("Failed to send initial batch");

        while blocks_received < TOTAL_BLOCKS {
            tokio::select! {
                res = manager_event_rx.recv() => match res {
                    Some(TorrentCommand::Block(..)) => {
                        blocks_received += 1;
                        if blocks_requested < TOTAL_BLOCKS {
                            let req = vec![(blocks_requested, 0, BLOCK_SIZE as u32)];
                            if client_cmd_tx.send(TorrentCommand::BulkRequest(req)).await.is_err() {
                                break; // Session dead
                            }
                            blocks_requested += 1;
                        }
                        if blocks_received % 5000 == 0 {
                            println!("Progress: {}/{}", blocks_received, TOTAL_BLOCKS);
                        }
                    },
                    Some(_) => continue,
                    None => {
                        println!("!!! SESSION DIED PREMATURELY - Awaiting Handle for Panic Info !!!");
                        // Await the handle to print the panic message from the spawned task
                        if let Err(e) = session_handle.await {
                            if e.is_panic() {
                                std::panic::resume_unwind(e.into_panic());
                            } else {
                                panic!("Session task cancelled or failed: {:?}", e);
                            }
                        }
                        panic!("Session closed manager channel but exited cleanly?");
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    panic!("Stalled: No blocks received for 10s");
                }
            }
        }

        // Assert success
        assert_eq!(blocks_received, TOTAL_BLOCKS);
        let elapsed = start_time.elapsed();
        let mb = (TOTAL_BLOCKS as f64 * BLOCK_SIZE as f64) / 1024.0 / 1024.0;
        println!(
            "DONE: {:.2} MB in {:.2?} ({:.2} MB/s)",
            mb,
            elapsed,
            mb / elapsed.as_secs_f64()
        );
    }

    // =========================================================================
    // TEST 1: ROCKET (Growth to Max)
    // =========================================================================
    #[tokio::test]
    async fn test_dynamic_window_growth_to_max() {
        let (mut network, client_cmd_tx, mut manager_event_rx, window_monitor) =
            spawn_test_session().await;
        perform_handshake(&mut network).await;

        // Mock Peer: Instant Responder
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let dummy_data = vec![0xAA; 16384];
            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await
            {
                match msg {
                    Message::Interested => {
                        let _ = peer_write
                            .write_all(&generate_message(Message::Unchoke).unwrap())
                            .await;
                    }
                    Message::Request(i, b, _) => {
                        // FIX: Add small delay to simulate network latency and force queue buildup
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        let piece =
                            generate_message(Message::Piece(i, b, dummy_data.clone())).unwrap();
                        let _ = peer_write.write_all(&piece).await;
                    }
                    _ => {}
                }
            }
        });

        let _ = client_cmd_tx.send(TorrentCommand::ClientInterested).await;
        loop {
            match timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Unchoke(_))) => break,
                Ok(Some(_)) => continue,
                _ => panic!("Startup failed"),
            }
        }

        // Blast Data
        let mut completed = 0;
        let mut inflight = 0;

        // We need enough duration/blocks for the 3s timer to tick multiple times
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) && completed < 1000 {
            while inflight < 150 {
                let _ = client_cmd_tx
                    .send(TorrentCommand::BulkRequest(vec![(
                        completed + inflight,
                        0,
                        16384,
                    )]))
                    .await;
                inflight += 1;
            }

            if let Ok(Some(TorrentCommand::Block(..))) =
                timeout(Duration::from_millis(100), manager_event_rx.recv()).await
            {
                completed += 1;
                if inflight > 0 {
                    inflight = inflight.saturating_sub(1);
                }
            }
        }

        let final_window = window_monitor.load(Ordering::Relaxed);
        println!("Rocket Test: Final Window Size = {}", final_window);

        assert!(
            final_window > PEER_BLOCK_IN_FLIGHT_LIMIT,
            "Window should have grown (Current: {}, Start: {})",
            final_window,
            PEER_BLOCK_IN_FLIGHT_LIMIT
        );
    }

    // =========================================================================
    // TEST 2: CONGESTION (Increase then Decrease)
    // =========================================================================
    #[tokio::test]
    async fn test_dynamic_window_congestion_control() {
        let (mut network, client_cmd_tx, mut manager_event_rx, window_monitor) =
            spawn_test_session().await;
        perform_handshake(&mut network).await;

        let is_congested = Arc::new(AtomicBool::new(false));
        let is_congested_clone = is_congested.clone();

        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let dummy_data = vec![0xAA; 16384];
            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await
            {
                match msg {
                    Message::Interested => {
                        let _ = peer_write
                            .write_all(&generate_message(Message::Unchoke).unwrap())
                            .await;
                    }
                    Message::Request(i, b, _) => {
                        if is_congested_clone.load(Ordering::Relaxed) {
                            // SIMULATE CONGESTION: 50ms delay per block (approx 320KB/s)
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        } else {
                            // FAST MODE: 2ms (Needed to trigger 'saturated' check)
                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                        let piece =
                            generate_message(Message::Piece(i, b, dummy_data.clone())).unwrap();
                        let _ = peer_write.write_all(&piece).await;
                    }
                    _ => {}
                }
            }
        });

        let _ = client_cmd_tx.send(TorrentCommand::ClientInterested).await;
        loop {
            if let Ok(Some(TorrentCommand::Unchoke(_))) =
                timeout(Duration::from_secs(1), manager_event_rx.recv()).await
            {
                break;
            }
        }

        // PHASE 1: GROW
        // We must run for at least >3s to allow the adjustment timer to tick.
        let start = Instant::now();
        let mut request_id = 0;
        let mut inflight: usize = 0;

        while start.elapsed() < Duration::from_secs(5) {
            // Keep pipe full to ensure saturation logic triggers
            while inflight < 150 {
                let _ = client_cmd_tx
                    .send(TorrentCommand::BulkRequest(vec![(request_id, 0, 16384)]))
                    .await;
                request_id += 1;
                inflight += 1;
            }

            // Consume blocks as they come in
            if let Ok(Some(TorrentCommand::Block(..))) =
                timeout(Duration::from_millis(50), manager_event_rx.recv()).await
            {
                if inflight > 0 {
                    inflight = inflight.saturating_sub(1);
                }
            }
        }

        let peak_window = window_monitor.load(Ordering::Relaxed);
        println!("Phase 1 Peak Window: {}", peak_window);
        assert!(
            peak_window > PEER_BLOCK_IN_FLIGHT_LIMIT,
            "Window failed to grow"
        );

        // PHASE 2: TRIGGER CONGESTION
        is_congested.store(true, Ordering::Relaxed);

        // Run for another 5 seconds to allow reaction
        let start_p2 = Instant::now();
        while start_p2.elapsed() < Duration::from_secs(5) {
            while inflight < 150 {
                let _ = client_cmd_tx
                    .send(TorrentCommand::BulkRequest(vec![(request_id, 0, 16384)]))
                    .await;
                request_id += 1;
                inflight += 1;
            }

            if let Ok(Some(TorrentCommand::Block(..))) =
                timeout(Duration::from_millis(50), manager_event_rx.recv()).await
            {
                if inflight > 0 {
                    inflight = inflight.saturating_sub(1);
                }
            }
        }

        let final_window = window_monitor.load(Ordering::Relaxed);
        println!("Phase 2 Final Window: {}", final_window);

        assert!(
            final_window < peak_window,
            "Window failed to shrink on congestion (Peak: {}, Final: {})",
            peak_window,
            final_window
        );
    }

    // =========================================================================
    // TEST 3: SUSTAIN (Steady State)
    // =========================================================================
    #[tokio::test]
    async fn test_dynamic_window_steady_state() {
        let (mut network, client_cmd_tx, mut manager_event_rx, window_monitor) =
            spawn_test_session().await;
        perform_handshake(&mut network).await;

        // Mock Peer: Fixed Rate (10ms delay)
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let dummy_data = vec![0xAA; 16384];
            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await
            {
                match msg {
                    Message::Interested => {
                        let _ = peer_write
                            .write_all(&generate_message(Message::Unchoke).unwrap())
                            .await;
                    }
                    Message::Request(i, b, _) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        let piece =
                            generate_message(Message::Piece(i, b, dummy_data.clone())).unwrap();
                        let _ = peer_write.write_all(&piece).await;
                    }
                    _ => {}
                }
            }
        });

        let _ = client_cmd_tx.send(TorrentCommand::ClientInterested).await;
        loop {
            if let Ok(Some(TorrentCommand::Unchoke(_))) =
                timeout(Duration::from_secs(1), manager_event_rx.recv()).await
            {
                break;
            }
        }

        // Run for a longer duration to check stability
        let mut completed = 0;
        let mut inflight = 0;

        // Process ~400 blocks (should take ~4 seconds minimum purely by delay, likely more)
        while completed < 400 {
            // Keep pipe full
            while inflight < 100 {
                let _ = client_cmd_tx
                    .send(TorrentCommand::BulkRequest(vec![(
                        completed + inflight,
                        0,
                        16384,
                    )]))
                    .await;
                inflight += 1;
            }

            if let Some(TorrentCommand::Block(..)) = manager_event_rx.recv().await {
                completed += 1;
                if inflight > 0 {
                    inflight = inflight.saturating_sub(1);
                }
            }
        }
        let final_window = window_monitor.load(Ordering::Relaxed);
        println!("Steady State Window: {}", final_window);

        assert!(
            final_window >= PEER_BLOCK_IN_FLIGHT_LIMIT,
            "Window collapsed unexpectedly"
        );
        assert!(final_window < 255, "Window overflowed");
    }

    #[tokio::test]
    async fn test_dynamic_window_reset_on_choke() {
        let (mut network, client_cmd_tx, mut manager_event_rx, window_monitor) =
            spawn_test_session().await;
        perform_handshake(&mut network).await;

        // Shared state to tell the Mock Peer when to Choke
        let should_choke = Arc::new(AtomicBool::new(false));
        let should_choke_clone = should_choke.clone();

        // Mock Peer Task
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let mut am_choking = true;
            let dummy_data = vec![0xAA; 16384];

            while let Ok(Ok(msg)) =
                timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await
            {
                // Check if the test controller wants us to choke
                if should_choke_clone.load(Ordering::Relaxed) && !am_choking {
                    let choke_msg = generate_message(Message::Choke).unwrap();
                    let _ = peer_write.write_all(&choke_msg).await;
                    // We stay choked for a bit to let the test verify
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    // Then we Unchoke
                    let unchoke_msg = generate_message(Message::Unchoke).unwrap();
                    let _ = peer_write.write_all(&unchoke_msg).await;
                    am_choking = false;
                    should_choke_clone.store(false, Ordering::Relaxed); // Reset trigger
                }

                match msg {
                    Message::Interested => {
                        if am_choking {
                            let unchoke = generate_message(Message::Unchoke).unwrap();
                            let _ = peer_write.write_all(&unchoke).await;
                            am_choking = false;
                        }
                    }
                    Message::Request(i, b, _) => {
                        // If we are currently choked, we ignore requests (simulate real peer)
                        if !am_choking {
                            // Fast response to encourage window growth
                            let piece =
                                generate_message(Message::Piece(i, b, dummy_data.clone())).unwrap();
                            let _ = peer_write.write_all(&piece).await;
                        }
                    }
                    _ => {}
                }
            }
        });

        // 1. Start Session
        let _ = client_cmd_tx.send(TorrentCommand::ClientInterested).await;
        loop {
            if let Ok(Some(TorrentCommand::Unchoke(_))) =
                timeout(Duration::from_secs(1), manager_event_rx.recv()).await
            {
                break;
            }
        }

        // 2. Grow the Window (Ramp Up)
        let mut completed = 0;
        let mut inflight = 0;
        let start = Instant::now();

        // Pump blocks for 4 seconds to trigger growth
        while start.elapsed() < Duration::from_secs(8) {
            while inflight < 100 {
                let _ = client_cmd_tx
                    .send(TorrentCommand::BulkRequest(vec![(
                        completed + inflight,
                        0,
                        16384,
                    )]))
                    .await;
                inflight += 1;
            }

            if let Ok(Some(TorrentCommand::Block(..))) =
                timeout(Duration::from_millis(50), manager_event_rx.recv()).await
            {
                if inflight > 0 {
                    inflight = inflight.saturating_sub(1);
                }
                completed += 1;
            }
        }

        let peak_window = window_monitor.load(Ordering::Relaxed);
        println!("Peak Window before Choke: {}", peak_window);

        assert!(
            peak_window > PEER_BLOCK_IN_FLIGHT_LIMIT,
            "Window did not grow enough to test reset (Got {}, want > {})",
            peak_window,
            PEER_BLOCK_IN_FLIGHT_LIMIT
        );

        // 3. Trigger Choke
        println!("Triggering Peer Choke...");
        should_choke.store(true, Ordering::Relaxed);

        // Wait for the Choke event to propagate
        // We look for the Manager to receive the 'Choke' command from the session
        loop {
            match timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Choke(_))) => break,
                Ok(Some(_)) => continue, // Ignore other messages
                _ => panic!("Did not receive Choke command from session"),
            }
        }

        // 4. ASSERT: Window should be reset immediately
        let choked_window = window_monitor.load(Ordering::Relaxed);
        println!("Window after Choke: {}", choked_window);

        assert_eq!(
            choked_window, PEER_BLOCK_IN_FLIGHT_LIMIT,
            "Window failed to reset to default on Choke!"
        );

        // 5. Verify Unchoke / Resumption
        // The mock peer is programmed to unchoke automatically after 500ms
        loop {
            match timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Unchoke(_))) => break,
                Ok(Some(_)) => continue,
                _ => panic!("Did not receive Unchoke command"),
            }
        }

        println!("Received Unchoke, test passed.");
    }
}
