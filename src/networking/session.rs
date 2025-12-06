// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_file::Info;
use crate::torrent_file::Torrent;

use super::protocol::{
    parse_message, writer_task, BlockInfo, ClientExtendedId,
    ExtendedHandshakePayload, Message, MessageSummary, MetadataMessage,
};

use std::collections::VecDeque;

#[cfg(feature = "pex")]
use super::protocol::PexMessage;

use crate::token_bucket::consume_tokens;
use crate::token_bucket::TokenBucket;

use crate::command::TorrentCommand;

use crate::torrent_manager::state::MAX_PIPELINE_DEPTH;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::sync::OwnedSemaphorePermit;
use tokio::io::split;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio::time::Duration;
use tokio::time::Instant;

use tracing::{event, instrument, Level};

const PEER_BLOCK_IN_FLIGHT_LIMIT: usize = 4;

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
    pub global_dl_bucket: Arc<Mutex<TokenBucket>>,
    pub global_ul_bucket: Arc<Mutex<TokenBucket>>,
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

    writer_rx: Receiver<Message>,
    writer_tx: Sender<Message>,

    // Tracks blocks IN FLIGHT. Used to credit permits back on receipt/cancel.
    //block_tracker: HashMap<u32, HashSet<BlockInfo>>,
    block_tracker: HashSet<BlockInfo>,
    
    // The "Gatekeeper". We control permits manually for max speed.
    block_request_limit_semaphore: Arc<Semaphore>,
    
    // Tracks spawn handles for graceful shutdown
    block_request_joinset: JoinSet<()>,
    
    block_upload_limit_semaphore: Arc<Semaphore>,

    peer_extended_id_mappings: HashMap<String, u8>,
    peer_extended_handshake_payload: Option<ExtendedHandshakePayload>,
    peer_torrent_metadata_piece_count: usize,
    peer_torrent_metadata_pieces: Vec<u8>,

    global_dl_bucket: Arc<Mutex<TokenBucket>>,
    global_ul_bucket: Arc<Mutex<TokenBucket>>,

    shutdown_tx: broadcast::Sender<()>,

    cancellation_buffer: Arc<Mutex<VecDeque<(u32, u32)>>>,
}

impl PeerSession {

    pub fn new(params: PeerSessionParameters) -> Self {
        // Increase channel size to buffer the Manager's "shotgun" blasts of requests
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
            writer_rx,
            writer_tx,
            block_tracker: HashSet::new(),
            // Semaphore matches pipeline depth
            block_request_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),
            block_request_joinset: JoinSet::new(),
            block_upload_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),
            peer_extended_id_mappings: HashMap::new(),
            peer_extended_handshake_payload: None,
            peer_torrent_metadata_piece_count: 0,
            peer_torrent_metadata_pieces: Vec::new(),
            global_dl_bucket: params.global_dl_bucket,
            global_ul_bucket: params.global_ul_bucket,
            shutdown_tx: params.shutdown_tx,
            cancellation_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_PIPELINE_DEPTH))),
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

        let (mut stream_read_half, stream_write_half) = split(stream);
        let (error_tx, mut error_rx) = oneshot::channel();

        let global_ul_bucket_clone = self.global_ul_bucket.clone();
        let writer_shutdown_rx = self.shutdown_tx.subscribe();
        let writer_handle = tokio::spawn(writer_task(
            stream_write_half,
            self.writer_rx,
            error_tx,
            global_ul_bucket_clone,
            writer_shutdown_rx,
        ));
        let _writer_abort_guard = AbortOnDrop(writer_handle);

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

        // TODO: Remove duplicate processing
        let peer_info_hash = &handshake_response[28..48];
        if self.info_hash != peer_info_hash {
            event!(
                Level::DEBUG,
                "Our hash: {:?} - peer hash: {:?}",
                self.info_hash,
                peer_info_hash
            );
            return Err("Info hash mismatch with peer".into());
        }

        let peer_id = handshake_response[48..68].to_vec();
        let _ = self
            .torrent_manager_tx
            .try_send(TorrentCommand::PeerId(self.peer_ip_port.clone(), peer_id));

        let reserved_bytes = &handshake_response[20..28];
        const EXTENSION_FLAG: u8 = 0x10;
        let peer_supports_extended = (reserved_bytes[5] & EXTENSION_FLAG) != 0;
        if peer_supports_extended {
            let mut torrent_metadata_len = None;
            if let Some(torrent_metadata_length) = self.torrent_metadata_length {
                torrent_metadata_len = Some(torrent_metadata_length);
            }
            // TODO: Send len back into session manager
            let _ = self
                .writer_tx
                .try_send(Message::ExtendedHandshake(torrent_metadata_len));
        }

        if let Some(bitfield) = current_bitfield {
            self.peer_session_established = true;
            let _ = self.writer_tx.try_send(Message::Bitfield(bitfield.clone()));
            let _ = self
                .torrent_manager_tx
                .try_send(TorrentCommand::SuccessfullyConnected(
                    self.peer_ip_port.clone(),
                ));
        }

        let mut keep_alive_timer = tokio::time::interval(Duration::from_secs(60));

        let inactivity_timeout = tokio::time::sleep(Duration::from_secs(120));
        tokio::pin!(inactivity_timeout);

        let _result: Result<(), Box<dyn StdError + Send + Sync>> = 'session: loop {
            event!(Level::DEBUG, "Peer session loop running");
            const READ_TIMEOUT: Duration = Duration::from_secs(120);

            tokio::select! {
                _ = &mut inactivity_timeout => {
                    event!(Level::DEBUG, "Peer timed out due to inactivity. Disconnecting.");
                    break 'session Err("Peer connection timed out".into());
                },

                _ = keep_alive_timer.tick() => {
                        let _ = self.writer_tx
                            .try_send(Message::KeepAlive) ;
                    event!(Level::TRACE, "Sent periodic Keep-Alive.");
                },

                Ok(message_from_peer) = timeout(READ_TIMEOUT, parse_message(&mut stream_read_half)) => {
                    if let Ok(ref message) = message_from_peer {
                        inactivity_timeout.as_mut().reset(Instant::now() + Duration::from_secs(120));
                        match message {
                            Message::KeepAlive => {
                                event!(Level::TRACE, ?message);
                            }
                            _ => {
                                event!(Level::TRACE, ?message);
                                event!(Level::DEBUG, message_summary = ?MessageSummary(message));
                            }
                        }
                    }

                    match message_from_peer {
                        Ok(Message::Bitfield(value)) => {
                            let _ = self.torrent_manager_tx
                                .try_send(TorrentCommand::PeerBitfield(self.peer_ip_port.clone(), value));
                        }
                        Ok(Message::NotInterested) => {}
                        Ok(Message::Interested) => {
                                let _ =
                                    self.torrent_manager_tx
                                    .try_send(TorrentCommand::PeerInterested(self.peer_ip_port.clone()));
                        }
                        Ok(Message::Choke) => {
                            self.block_tracker.clear();
                            let max = PEER_BLOCK_IN_FLIGHT_LIMIT;
                            let current = self.block_request_limit_semaphore.available_permits();
                            if current < max {
                                self.block_request_limit_semaphore.add_permits(max - current);
                            }
                        }
                        Ok(Message::Unchoke) => {
                                let _ =
                                    self.torrent_manager_tx
                                    .try_send(TorrentCommand::Unchoke(self.peer_ip_port.clone()));
                        },


                        Ok(Message::Piece(piece_index, block_offset, block_data)) => {
                            if self.block_request_limit_semaphore.available_permits() < PEER_BLOCK_IN_FLIGHT_LIMIT {
                                self.block_request_limit_semaphore.add_permits(1);
                            }

                            let peer_ip_port_clone = self.peer_ip_port.clone();
                            let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
                            let mut shutdown_rx = self.shutdown_tx.subscribe();
                            

                            let capacity = torrent_manager_tx_clone.capacity();
                            let max_cap = torrent_manager_tx_clone.max_capacity();
                            if capacity < (max_cap / 20) { // Log if less than 10% free
                                event!(
                                    Level::WARN, 
                                    "⚠️  MANAGER CHANNEL CONGESTION: {}/{} permits remaining. Manager is lagging!", 
                                    capacity, max_cap
                                );
                            }

                            tokio::spawn(async move {
                                let cmd = TorrentCommand::Block(
                                    peer_ip_port_clone.clone(), 
                                    piece_index, 
                                    block_offset, 
                                    block_data,
                                );

                                tokio::select! {
                                     res = torrent_manager_tx_clone.send(cmd) => {
                                         if let Err(e) = res {
                                             event!(Level::ERROR, "Manager channel closed unexpectedly: {}", e);
                                         }
                                     }
                                     _ = shutdown_rx.recv() => {}
                                }
                            });
                        },

                        Ok(Message::Request(piece_index, block_offset, block_length)) => {
                                let _ = self.torrent_manager_tx
                                    .try_send(TorrentCommand::RequestUpload(self.peer_ip_port.clone(), piece_index, block_offset, block_length));
                        },
                        Ok(Message::Cancel(piece_index, block_offset, block_length)) => {
                                let _ = self.torrent_manager_tx
                                    .try_send(TorrentCommand::CancelUpload(self.peer_ip_port.clone(), piece_index, block_offset, block_length));
                        },
                        Ok(Message::Have(piece_index)) => {
                                let _ = self.torrent_manager_tx
                                    .try_send(TorrentCommand::Have(self.peer_ip_port.clone(), piece_index));
                        }
                        Ok(Message::Extended(extended_id, payload)) => {

                            if extended_id == ClientExtendedId::Handshake.id() {
                                if let Ok(handshake_data) = serde_bencode::from_bytes::<ExtendedHandshakePayload>(&payload) {

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
                                                match serde_bencode::to_bytes(&request) {
                                                    Ok(payload_bytes) => {
                                                        let _ = self.writer_tx.try_send(
                                                            Message::Extended(ClientExtendedId::UtMetadata.id(), payload_bytes)
                                                        );
                                                    }
                                                    Err(e) => {
                                                        event!(Level::ERROR, "Failed to serialize metadata request: {}", e);
                                                    }
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
                                                let _ = self.torrent_manager_tx
                                                    .try_send(TorrentCommand::AddPexPeers(self.peer_ip_port.clone(), new_peers));
                                        }
                                    }
                                }
                            }
                            if extended_id == ClientExtendedId::UtMetadata.id()
                                && !self.peer_session_established {
                                    if let Some(ref handshake_data) = self.peer_extended_handshake_payload {
                                        if let Some(torrent_metadata_len) = handshake_data.metadata_size {
                                            let torrent_metadata_len_usize = torrent_metadata_len as usize;

                                            let current_offset = self.peer_torrent_metadata_piece_count * 16384;
                                            let expected_data_len = std::cmp::min(16384, torrent_metadata_len_usize - current_offset);
                                            let header_len = payload.len() - expected_data_len;
                                            let metadata_binary = &payload[header_len..];
                                            self.peer_torrent_metadata_pieces.extend(metadata_binary);

                                            if torrent_metadata_len_usize == self.peer_torrent_metadata_pieces.len() {

                                                let dht_info_result: Result<Info, _> = serde_bencode::from_bytes(&self.peer_torrent_metadata_pieces[..]);

                                                match dht_info_result {
                                                    Ok(dht_info) => {
                                                            let _ = self.torrent_manager_tx
                                                                .try_send(TorrentCommand::DhtTorrent(
                                                                    Torrent {
                                                                        info_dict_bencode: self.peer_torrent_metadata_pieces.clone(),
                                                                        info: dht_info,
                                                                        announce: None,
                                                                        announce_list: None,
                                                                        url_list: None,
                                                                        creation_date: None,
                                                                        comment: None,
                                                                        created_by: None,
                                                                        encoding: None
                                                                    },
                                                                    torrent_metadata_len
                                                                ));
                                                    }
                                                    Err(e) => {
                                                        event!(Level::WARN, "Failed to decode torrent metadata from peer: {}", e);
                                                        return Err("Peer sent invalid torrent metadata".into());
                                                    }
                                                }

                                            } else {
                                                self.peer_torrent_metadata_piece_count += 1;
                                                let request = MetadataMessage {
                                                    msg_type: 0,
                                                    piece: self.peer_torrent_metadata_piece_count,
                                                    total_size: None,
                                                };
                                                match serde_bencode::to_bytes(&request) {
                                                    Ok(payload_bytes) => {
                                                        let _ = self.writer_tx.try_send(
                                                            Message::Extended(ClientExtendedId::UtMetadata.id(), payload_bytes)
                                                        );
                                                    }
                                                    Err(e) => {
                                                        event!(Level::ERROR, "Failed to serialize metadata request: {}", e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                        }
                        Ok(Message::KeepAlive) => {
                            event!(Level::TRACE, "PEER KEEP SENT KEEP ALIVE.");
                        }
                        Ok(Message::Port(_)) => {}
                        Err(e) => {
                            break 'session Err(e.into());
                        }
                        _ => {
                            event!(Level::WARN, "UNIMPLEMENTED MESSAGE {:?}", message_from_peer);
                        },
                    }
                },

                Some(command) = self.torrent_manager_rx.recv() => {
                    event!(Level::TRACE, ?command);
                    match command {
                        #[cfg(feature = "pex")]
                        TorrentCommand::SendPexPeers(peers_list) => {
                            if let Some(pex_id) = self.peer_extended_id_mappings.get(ClientExtendedId::UtPex.as_str()).copied() {
                                let pex_list_for_this_peer: Vec<u8> = peers_list.iter()
                                    .filter(|&peer_ip| *peer_ip != self.peer_ip_port)
                                    .filter_map(|ip_port| ip_port.parse::<std::net::SocketAddr>().ok())
                                    .filter_map(|addr| {
                                        if let std::net::SocketAddr::V4(v4_addr) = addr {
                                            let mut peer_bytes = Vec::with_capacity(6);
                                            peer_bytes.extend_from_slice(&v4_addr.ip().octets());
                                            peer_bytes.extend_from_slice(&v4_addr.port().to_be_bytes());
                                            Some(peer_bytes)
                                        } else {
                                            None
                                        }
                                    })
                                    .flatten()
                                    .collect();

                                if pex_list_for_this_peer.is_empty() {
                                    continue;
                                }

                                let pex_message = PexMessage {
                                    added: pex_list_for_this_peer,
                                    ..Default::default()
                                };

                                if let Ok(bencoded_payload) = serde_bencode::to_bytes(&pex_message) {
                                    let _ = self.writer_tx.try_send(
                                        Message::Extended(pex_id, bencoded_payload)
                                    );
                                }
                            }

                        }
                        TorrentCommand::PeerUnchoke => {
                                let _ = self.writer_tx
                                    .try_send(Message::Unchoke);
                        }
                        TorrentCommand::PeerChoke => {
                                let _ = self.writer_tx
                                    .try_send(Message::Choke);
                        }
                        TorrentCommand::Disconnect(_) => {
                            break 'session Err("DISCONNECTING PEER".into());
                        }
                        TorrentCommand::ClientInterested => {
                                let _ = self.writer_tx
                                    .try_send(Message::Interested);
                        }
                        TorrentCommand::NotInterested => {
                                let _ = self.writer_tx
                                    .try_send(Message::NotInterested);
                        }

                        TorrentCommand::Cancel(piece_index, block_offset, length) => {
                            let writer_tx_clone = self.writer_tx.clone();
                            let cancel_buffer = self.cancellation_buffer.clone();
                            let mut shutdown_rx = self.shutdown_tx.subscribe();

                            tokio::spawn(async move {
                                // 1. Log the cancel efficiently
                                {
                                    let mut buf = cancel_buffer.lock().await;
                                    if buf.len() >= 50 { buf.pop_front(); }
                                    buf.push_back((piece_index, block_offset));
                                }

                                // 2. Send the wire message
                                tokio::select! {
                                    _ = writer_tx_clone.send(Message::Cancel(piece_index, block_offset, length)) => {},
                                    _ = shutdown_rx.recv() => {}
                                }
                            });
                        },

                        TorrentCommand::Have(_peer_id, piece_index) => {
                                let _ = self.writer_tx
                                    .try_send(Message::Have(piece_index));
                        }

                        TorrentCommand::SendRequest { index, begin, length } => {
                            let writer_tx_clone = self.writer_tx.clone();
                            let semaphore_clone = self.block_request_limit_semaphore.clone();
                            let cancel_buffer = self.cancellation_buffer.clone();
                            let mut shutdown_rx = self.shutdown_tx.subscribe();



                            let capacity = writer_tx_clone.capacity();
                            let max_cap = writer_tx_clone.max_capacity();
                            if capacity < (max_cap / 20) { // Log if less than 10% free
                                event!(
                                    Level::WARN, 
                                    "⚠️  WRITER CHANNEL CONGESTION: {}/{} permits remaining. WRITER is lagging!", 
                                    capacity, max_cap
                                );
                            }
                            tokio::spawn(async move {
                                let permit = tokio::select! {
                                    Ok(p) = semaphore_clone.acquire_owned() => p,
                                    _ = shutdown_rx.recv() => return,
                                };

                                {
                                    let buf = cancel_buffer.lock().await;
                                    // .contains is linear search, but for 50 items it's faster than a Hash
                                    if buf.contains(&(index, begin)) {
                                        return; // Drop permit, exit. Saved bandwidth!
                                    }
                                }

                                tokio::select! {
                                    res = writer_tx_clone.send(Message::Request(index, begin, length)) => {
                                        match res {
                                            Ok(_) => {
                                                permit.forget();
                                            }
                                            Err(_) => {
                                                event!(Level::DEBUG, "Connection closed while sending request");
                                            }
                                        }
                                    }
                                    _ = shutdown_rx.recv() => {
                                    }
                                }

                            });
                        },
                        TorrentCommand::Upload(piece_index, block_offset, block_data) => {
                            let writer_tx_clone = self.writer_tx.clone();
                            let mut shutdown_rx = self.shutdown_tx.subscribe();

                            tokio::spawn(async move {
                                let msg = Message::Piece(piece_index, block_offset, block_data);
                                
                                tokio::select! {
                                    res = writer_tx_clone.send(msg) => {
                                        if let Err(_) = res {
                                            event!(Level::DEBUG, "Writer channel closed, upload canceled.");
                                        }
                                    }
                                    _ = shutdown_rx.recv() => {
                                    }
                                }
                            });
                        }
                        _ => {
                            event!(Level::WARN, "UNIMPLEMENTED TORRENT COMMAND {:?}", command);
                        }
                    }
                }

                writer_error = &mut error_rx => {
                    match writer_error {
                        Ok(err) => {
                            break 'session Err(err);
                        },
                        Err(_) => {
                            break 'session Err("Writer task panicked or channel closed unexpectedly.".into());
                        }
                    }
                },

                else => {
                    event!(Level::INFO, "ALL CHANNELS CLOSED FOR PEER");
                    break 'session Err("ALL CHANNELS CLOSED".into());
                }
            }
        };

        if !self.block_request_joinset.is_empty() {
            event!(Level::DEBUG, "Waiting for {} pending block tasks to finish...", self.block_request_joinset.len());
            while self.block_request_joinset.join_next().await.is_some() {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::protocol::{generate_message, parse_message, Message};
    use proptest::collection;
    use proptest::prelude::ProptestConfig;
    use proptest::proptest;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{broadcast, mpsc, Mutex};

    // --- Helper: Setup a Session with a Virtual Pipe ---
    async fn spawn_test_session() -> (
        tokio::io::DuplexStream,        // The "Network" side (Mock Peer)
        mpsc::Sender<TorrentCommand>,   // Channel to command the Client (Manager -> Session)
        mpsc::Receiver<TorrentCommand>, // Channel to receive events from Client (Session -> Manager)
    ) {
        // 1. Create the Virtual Network Pipe (64KB buffer)
        let (client_socket, mock_peer_socket) = duplex(64 * 1024);

        // 2. Mock Global Resources (Infinite Bandwidth)
        let infinite_bucket = Arc::new(Mutex::new(TokenBucket::new(f64::INFINITY, f64::INFINITY)));
        let (manager_tx, manager_rx) = mpsc::channel(100);
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (shutdown_tx, _) = broadcast::channel(1);

        // 3. Setup Session Parameters
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

        // 4. Spawn the Client Session
        tokio::spawn(async move {
            let session = PeerSession::new(params);
            if let Err(e) = session.run(client_socket, vec![], Some(vec![])).await {
                eprintln!("Test Session finished: {:?}", e);
            }
        });

        (mock_peer_socket, cmd_tx, manager_rx)
    }

    #[tokio::test]
    async fn test_pipeline_saturation_with_virtual_time() {
        let (mut network, client_cmd_tx, _manager_event_rx) = spawn_test_session().await;

        // --- Step 1: Handshake (Standard Protocol) ---
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.expect("Failed to read client handshake");

        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.expect("Failed to write handshake");

        // Consume Initial Messages
        for _ in 0..5 {
            if let Ok(msg) = timeout(Duration::from_millis(100), parse_message(&mut network)).await {
                if let Ok(Message::Bitfield(_)) = msg { break; }
            }
        }

        // --- Step 2: The Saturation Test (Pipeline Pressure) ---
        // NEW: Manager (Test) sends 5 distinct requests directly
        for i in 0..5 {
            client_cmd_tx
                .send(TorrentCommand::SendRequest {
                    index: 0,
                    begin: i * 16384,
                    length: 16384
                })
                .await
                .expect("Failed to send command");
        }

        // ASSERTION 1: Immediate Burst
        let mut requests_received = HashSet::new();
        let start = std::time::Instant::now();
        
        while requests_received.len() < 5 {
            if start.elapsed() > Duration::from_millis(500) {
                panic!("Timed out waiting for requests. Got: {}", requests_received.len());
            }

            let msg = timeout(Duration::from_millis(100), parse_message(&mut network))
                .await.expect("Stalled").expect("Parse fail");

            if let Message::Request(idx, begin, len) = msg {
                assert_eq!(idx, 0);
                assert_eq!(len, 16384);
                requests_received.insert(begin);
            }
        }
        assert_eq!(requests_received.len(), 5);
    }

    #[tokio::test]
    async fn test_fragmented_pipeline_saturation() {
        let (mut network, client_cmd_tx, _manager_event_rx) = spawn_test_session().await;

        // ... (Handshake setup omitted for brevity, assume same as above) ...
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.unwrap();
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.unwrap();
        for _ in 0..5 {
            if let Ok(Ok(Message::Bitfield(_))) = timeout(Duration::from_millis(100), parse_message(&mut network)).await { break; }
        }

        // NEW: Send 5 separate commands for 5 separate pieces
        for i in 0..5 {
            client_cmd_tx
                .send(TorrentCommand::SendRequest {
                    index: i as u32,
                    begin: 0,
                    length: 16384
                })
                .await
                .expect("Failed to send command");
        }

        let mut requested_pieces = HashSet::new();
        let start = std::time::Instant::now();
        
        while requested_pieces.len() < 5 {
            if start.elapsed() > Duration::from_millis(500) { panic!("Timeout"); }
            if let Ok(Ok(Message::Request(idx, _, _))) = timeout(Duration::from_millis(100), parse_message(&mut network)).await {
                requested_pieces.insert(idx);
            }
        }
        assert_eq!(requested_pieces.len(), 5);
    }

    #[tokio::test]
    async fn test_performance_1000_blocks_sliding_window() {
        // 1. Setup Session
        let (mut network, client_cmd_tx, mut manager_event_rx) = spawn_test_session().await;

        // 2. Handshake
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.expect("Handshake read failed");
        
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.expect("Handshake write failed");

        // 3. Spawn the "Smart Peer"
        // CHANGE: Now handles Bitfield, Interested, and Request correctly
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        
        tokio::spawn(async move {
            let mut am_choking = true;

            loop {
                // Generous timeout to prevent flakiness
                match timeout(Duration::from_secs(5), parse_message(&mut peer_read)).await {
                    Ok(Ok(msg)) => match msg {
                        // A. We must unchoke them when they ask
                        Message::Interested => {
                            if am_choking {
                                let unchoke = generate_message(Message::Unchoke).unwrap();
                                peer_write.write_all(&unchoke).await.unwrap();
                                am_choking = false;
                            }
                        }
                        // B. The main data loop
                        Message::Request(index, begin, _len) => {
                            if !am_choking {
                                let data = vec![1u8; 16384]; 
                                let piece = generate_message(Message::Piece(index, begin, data)).unwrap();
                                if peer_write.write_all(&piece).await.is_err() { break; }
                            }
                        }
                        // C. IGNORE Bitfield, Have, KeepAlive (Don't break!)
                        _ => {} 
                    },
                    _ => break, // Real error or timeout
                }
            }
        });

        // 4. Manager: Perform Startup Handshake
        // Wait for session to be ready
        let mut session_ready = false;
        while !session_ready {
            match timeout(Duration::from_secs(1), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::SuccessfullyConnected(_))) => session_ready = true,
                Ok(Some(TorrentCommand::PeerBitfield(_, _))) => session_ready = true,
                Ok(Some(_)) => continue, // Ignore PeerId etc.
                _ => panic!("Session failed to connect"),
            }
        }

        // Tell Session we want data
        client_cmd_tx.send(TorrentCommand::ClientInterested).await.unwrap();

        // Wait for Session to tell us we are Unchoked
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

        // 5. Run the Sliding Window Performance Test
        const TOTAL_BLOCKS: u32 = 1000;
        const WINDOW_SIZE: u32 = 20; // Keep 20 requests in flight
        const BLOCK_SIZE: usize = 16384;
        
        let start_time = Instant::now();
        let mut blocks_requested = 0;
        let mut blocks_received = 0;

        // Fill the window
        while blocks_requested < WINDOW_SIZE {
            client_cmd_tx.send(TorrentCommand::SendRequest {
                index: blocks_requested, begin: 0, length: BLOCK_SIZE as u32
            }).await.unwrap();
            blocks_requested += 1;
        }

        // Processing loop
        while blocks_received < TOTAL_BLOCKS {
            match timeout(Duration::from_secs(2), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Block(..))) => {
                    blocks_received += 1;

                    // Keep the pipeline full
                    if blocks_requested < TOTAL_BLOCKS {
                        client_cmd_tx.send(TorrentCommand::SendRequest {
                            index: blocks_requested, begin: 0, length: BLOCK_SIZE as u32
                        }).await.unwrap();
                        blocks_requested += 1;
                    }
                }
                Ok(Some(_)) => continue, // Ignore KeepAlives
                Ok(None) => panic!("Session died during transfer"),
                Err(_) => panic!("Stalled at {}/{}", blocks_received, TOTAL_BLOCKS),
            }
        }

        let elapsed = start_time.elapsed();
        let total_mb = (TOTAL_BLOCKS * BLOCK_SIZE as u32) as f64 / 1_000_000.0;
        
        println!("Success: {:.2} MB in {:.2?} ({:.2} MB/s)", 
            total_mb, elapsed, total_mb / elapsed.as_secs_f64());
    }

    // --- Helper for Custom Bucket (Matches your current struct) ---
    async fn spawn_custom_session(
        bucket: Arc<Mutex<TokenBucket>>,
    ) -> (
        tokio::io::DuplexStream,
        mpsc::Sender<TorrentCommand>,
        mpsc::Receiver<TorrentCommand>,
    ) {
        let (client, server) = tokio::io::duplex(1024 * 1024); // Large buffer for performance
        let (manager_tx, manager_rx) = mpsc::channel(2000);
        let (cmd_tx, cmd_rx) = mpsc::channel(1000);
        let (shutdown, _) = broadcast::channel(1);

        let params = PeerSessionParameters {
            info_hash: [0u8; 20].to_vec(),
            torrent_metadata_length: None,
            connection_type: ConnectionType::Outgoing,
            torrent_manager_rx: cmd_rx,
            torrent_manager_tx: manager_tx,
            peer_ip_port: "test".into(),
            client_id: b"-SS1000-000000000000".to_vec(),
            global_dl_bucket: bucket.clone(), // Custom Bucket
            global_ul_bucket: bucket.clone(),
            shutdown_tx: shutdown,
            // pipeline_limit removed (uses default const 5)
        };

        tokio::spawn(async move {
            let s = PeerSession::new(params);
            // CHANGE: Print the error so we can see why it died
            if let Err(e) = s.run(client, vec![], Some(vec![])).await {
                println!("\n\n!!! SESSION DIED WITH ERROR: {:?} !!!\n\n", e);
            }
        });

        (server, cmd_tx, manager_rx)
    }


    proptest! {
        #![proptest_config(ProptestConfig::default())]

        #[test]
        fn fuzz_token_wallet_stability(
            // 1. Random Rate Limit: 10 KB/s to 100 MB/s
            rate_limit in 10_000.0..100_000_000.0f64,
            // 2. Random "Jittery" Block Sizes: A vector of sizes from 1 byte to 16KB
            // We'll simulate a transfer of ~10 items
            block_sizes in collection::vec(1usize..16384, 10..20)
        ) {
            // We must create a new Runtime for each proptest iteration
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                // --- Setup ---
                let total_bytes: usize = block_sizes.iter().sum();
                let bucket = Arc::new(Mutex::new(TokenBucket::new(rate_limit, rate_limit)));

                let (mut network, client_cmd_tx, mut manager_event_rx) = spawn_custom_session(bucket.clone()).await;

                // Handshake
                let mut handshake = vec![0u8; 68];
                network.read_exact(&mut handshake).await.unwrap();
                let mut resp = vec![0u8; 68];
                resp[0] = 19;
                resp[1..20].copy_from_slice(b"BitTorrent protocol");
                network.write_all(&resp).await.unwrap();
                // Drain setup messages
                let _ = timeout(Duration::from_millis(50), parse_message(&mut network)).await;

                // --- Command Client ---
                // Tell client to expect the total size
                let block_sizes_clone = block_sizes.clone();
                tokio::spawn(async move {
                    let mut offset = 0;
                    for size in block_sizes_clone {
                        let _ = client_cmd_tx.send(TorrentCommand::SendRequest {
                            index: 0,
                            begin: offset,
                            length: size as u32
                        }).await;
                        offset += size as u32;
                    }
                });

                // --- Helper: Writer Task ---
                // Spawns a background task to write the weirdly sized blocks
                let (mut peer_read, mut peer_write) = tokio::io::split(network);

                // Drain requests
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = peer_read.read(&mut buf).await {
                        if n == 0 { break; }
                    }
                });

                let blocks_clone = block_sizes.clone();
                let writer = tokio::spawn(async move {
                    let mut offset = 0;
                    for size in blocks_clone {
                        let data = vec![0u8; size];
                        let msg = Message::Piece(0, offset, data);
                        let bytes = generate_message(msg).unwrap();
                        peer_write.write_all(&bytes).await.unwrap();
                        offset += size as u32;
                    }
                });

                // --- Measurement Loop ---
                let start = std::time::Instant::now();

                let mut received_count = 0;
                while received_count < block_sizes.len() {
                    // Generous timeout per block to avoid flakes on slow CI
                    match timeout(Duration::from_secs(5), manager_event_rx.recv()).await {
                        Ok(Some(TorrentCommand::Block(..))) => received_count += 1,
                        Ok(_) => {},
                        Err(_) => panic!("Stalled during fuzz test! Rate: {}, BlockSizes: {:?}", rate_limit, block_sizes),
                    }
                }

                writer.await.unwrap();
                let elapsed = start.elapsed().as_secs_f64();

                // --- Assertions ---

                // 1. Minimum Time: It MUST NOT be faster than the rate limit allows.
                // We subtract 1.0s to account for the initial bucket burst (bucket starts full).
                let ideal_time = total_bytes as f64 / rate_limit;

                // Only enforce rate limit if the transfer is large enough to drain the initial burst
                // Otherwise, it's instant, which is correct behavior for TokenBuckets.
                if total_bytes as f64 > rate_limit {
                     assert!(
                        elapsed >= (ideal_time - 1.0).max(0.0),
                        "Client cheated! Too fast. Rate: {}, Time: {}, Ideal: {}", rate_limit, elapsed, ideal_time
                    );
                }

                // 2. Maximum Time: It MUST NOT be egregiously slow (overhead).
                // We allow a 200ms buffer + 50% overhead margin for test runtime.
                // This detects if your batching logic accidentally sleeps for 5 seconds on small packets.
                let max_allowed = ideal_time * 1.5 + 2.0;
                assert!(
                    elapsed <= max_allowed,
                    "Client too slow/laggy! Rate: {}, Time: {}, Ideal: {}", rate_limit, elapsed, ideal_time
                );
            });
        }
    }
}
