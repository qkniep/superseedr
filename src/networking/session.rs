// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_file::Info;
use crate::torrent_file::Torrent;

use super::protocol::{
    calculate_blocks_for_piece, parse_message, writer_task, BlockInfo, ClientExtendedId,
    ExtendedHandshakePayload, Message, MessageSummary, MetadataMessage,
};

#[cfg(feature = "pex")]
use super::protocol::PexMessage;

use crate::token_bucket::consume_tokens;
use crate::token_bucket::TokenBucket;

use crate::command::TorrentCommand;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::io::AsyncReadExt;

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

const PEER_BLOCK_IN_FLIGHT_LIMIT: usize = 5;

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

    block_tracker: HashMap<u32, HashSet<BlockInfo>>,
    block_request_buffer: Vec<u8>,
    block_request_limit_semaphore: Arc<Semaphore>,
    block_request_joinset: JoinSet<()>,
    block_requests_remaining: usize,
    block_upload_limit_semaphore: Arc<Semaphore>,

    peer_extended_id_mappings: HashMap<String, u8>,
    peer_extended_handshake_payload: Option<ExtendedHandshakePayload>,
    peer_torrent_metadata_piece_count: usize,
    peer_torrent_metadata_pieces: Vec<u8>,

    local_download_stash: Arc<Mutex<f64>>,
    global_dl_bucket: Arc<Mutex<TokenBucket>>,
    global_ul_bucket: Arc<Mutex<TokenBucket>>,

    shutdown_tx: broadcast::Sender<()>,
}

impl PeerSession {
    pub fn new(params: PeerSessionParameters) -> Self {
        let (writer_tx, writer_rx) = mpsc::channel::<Message>(100);

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
            block_tracker: HashMap::new(),
            block_request_buffer: Vec::new(),
            block_request_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),
            block_request_joinset: JoinSet::new(),
            block_requests_remaining: 0,
            block_upload_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),
            peer_extended_id_mappings: HashMap::new(),
            peer_extended_handshake_payload: None,
            peer_torrent_metadata_piece_count: 0,
            peer_torrent_metadata_pieces: Vec::new(),
            local_download_stash: Arc::new(Mutex::new(0.0)),
            global_dl_bucket: params.global_dl_bucket,
            global_ul_bucket: params.global_ul_bucket,
            shutdown_tx: params.shutdown_tx,
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
            const READ_TIMEOUT: Duration = Duration::from_secs(2);

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
                                let _ =
                                    self.torrent_manager_tx
                                    .try_send(TorrentCommand::Choke(self.peer_ip_port.clone()));
                        }
                        Ok(Message::Unchoke) => {
                                let _ =
                                    self.torrent_manager_tx
                                    .try_send(TorrentCommand::Unchoke(self.peer_ip_port.clone()));
                        },
                        Ok(Message::Piece(piece_index, block_offset , block_data)) => {

                            let received_block = BlockInfo {
                                piece_index,
                                offset: block_offset,
                                length: block_data.len() as u32,
                            };

                            if let Entry::Occupied(mut entry) = self.block_tracker.entry(piece_index) {
                                let blocks_for_piece = entry.get_mut();
                                if blocks_for_piece.remove(&received_block) {
                                    self.block_request_limit_semaphore.add_permits(1);
                                }
                                if blocks_for_piece.is_empty() {
                                    entry.remove();
                                }
                            }

                            let peer_ip_port_clone = self.peer_ip_port.clone();
                            let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
                            let _block_request_buffer_clone = self.block_request_buffer.clone();
                            let global_dl_bucket_clone = self.global_dl_bucket.clone();

                            let local_stash_clone = self.local_download_stash.clone();

                            self.block_request_joinset.spawn(async move {
                                let len = block_data.len() as f64;

                                {
                                    let mut stash = local_stash_clone.lock().await;

                                    if *stash < len {
                                        let rate = {
                                            let b = global_dl_bucket_clone.lock().await;
                                            b.get_rate()
                                        };

                                        let target_duration = 0.2;
                                        let dynamic_batch = rate * target_duration;
                                        let batch_size = dynamic_batch.clamp(16384.0, 5.0 * 1024.0 * 1024.0);
                                        let amount_to_request = batch_size.max(len);

                                        consume_tokens(&global_dl_bucket_clone, amount_to_request).await;
                                        *stash += amount_to_request;
                                    }

                                    *stash -= len;
                                }

                                let _ = torrent_manager_tx_clone
                                    .send(TorrentCommand::Block(peer_ip_port_clone, piece_index, block_offset, block_data))
                                    .await;
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
                        TorrentCommand::Cancel(piece_index) => {
                            if let Some(blocks) = self.block_tracker.remove(&piece_index) {
                                for block in blocks {
                                    if self.block_request_limit_semaphore.available_permits() < PEER_BLOCK_IN_FLIGHT_LIMIT {
                                        self.block_request_limit_semaphore.add_permits(1);
                                    }

                                        let _ = self.writer_tx
                                            .try_send(Message::Cancel(
                                                block.piece_index,
                                                block.offset,
                                                block.length
                                            ));
                                }
                            }

                            // TODO: Wrap this joinset and blockinfo into one struct and drop both
                            self.block_request_joinset = JoinSet::new();

                        }
                        TorrentCommand::Have(_peer_id, piece_index) => {
                                let _ = self.writer_tx
                                    .try_send(Message::Have(piece_index));
                        }
                        TorrentCommand::RequestDownload(piece_index, piece_length, torrent_size) => {
                            let piece_start = piece_index as u64 * piece_length as u64;
                            let remaining_bytes = (torrent_size as u64).saturating_sub(piece_start);
                            let piece_size = std::cmp::min(piece_length as u64, remaining_bytes) as u32;

                            self.block_request_buffer = vec![0; piece_size as usize];

                            let blocks = calculate_blocks_for_piece(
                                piece_index,
                                piece_size
                            );
                            self.block_tracker.insert(piece_index, blocks.clone());
                            self.block_requests_remaining = blocks.len();
                            for block in blocks.into_iter() {
                                let writer_tx_clone = self.writer_tx.clone();
                                let semaphore_clone = self.block_request_limit_semaphore.clone();
                                let mut shutdown_rx = self.shutdown_tx.subscribe();
                                tokio::spawn(async move {
                                    let permit = tokio::select! {
                                        res = semaphore_clone.acquire() => {
                                            res.ok()
                                        }
                                        _ = shutdown_rx.recv() => {
                                            event!(Level::TRACE, "RequestDownload task cancelled by shutdown signal.");
                                            None
                                        }
                                    };

                                    if let Some(permit) = permit {
                                        let send_result = writer_tx_clone
                                            .try_send(Message::Request(
                                                block.piece_index,
                                                block.offset,
                                                block.length,
                                            ));
                                        if send_result.is_ok() {
                                            permit.forget();
                                        }
                                    }
                                });
                            }
                        }
                        TorrentCommand::Upload(piece_index, block_offset, block_data) => {
                            let writer_tx_clone = self.writer_tx.clone();
                            let _semaphore_clone = self.block_upload_limit_semaphore.clone();
                                let _ = writer_tx_clone
                                    .try_send(Message::Piece(
                                        piece_index,
                                        block_offset,
                                        block_data,
                                    ));
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
        network
            .read_exact(&mut handshake_buf)
            .await
            .expect("Failed to read client handshake");

        // Send Valid Handshake Response (With Extensions Enabled)
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]); // Extension bit set
        network
            .write_all(&response)
            .await
            .expect("Failed to write handshake");

        // Consume Initial Messages (Extended Handshake + Bitfield)
        let mut seen_bitfield = false;
        for _ in 0..5 {
            if let Ok(msg) = timeout(Duration::from_millis(100), parse_message(&mut network)).await
            {
                let msg = msg.expect("Failed to parse");
                if let Message::Bitfield(_) = msg {
                    seen_bitfield = true;
                    break;
                }
            }
        }
        assert!(seen_bitfield, "Client did not send Bitfield");

        // --- Step 2: The Saturation Test (Pipeline Pressure) ---

        // Command the client to download 10 blocks (More than limit of 5)
        let piece_len = 16384 * 10;
        client_cmd_tx
            .send(TorrentCommand::RequestDownload(
                0,
                piece_len as i64,
                piece_len as i64,
            ))
            .await
            .expect("Failed to send command");

        // ASSERTION 1: Immediate Burst
        println!("Checking for initial burst of 5 requests (Order Agnostic)...");
        let mut requests_received = HashSet::new();

        // We loop to collect exactly 5 REQUESTS.
        // If we see other messages (like Interested), we ignore them but keep looking.
        let start = std::time::Instant::now();
        while requests_received.len() < 5 {
            if start.elapsed() > Duration::from_millis(500) {
                panic!(
                    "Timed out waiting for 5 requests. Got: {}",
                    requests_received.len()
                );
            }

            let msg = timeout(Duration::from_millis(100), parse_message(&mut network))
                .await
                .expect("Client stalled! Pipeline burst incomplete.")
                .expect("Failed to parse message");

            match msg {
                Message::Request(idx, begin, len) => {
                    assert_eq!(idx, 0);
                    assert_eq!(len, 16384);
                    requests_received.insert(begin);
                }
                _ => {
                    println!("Note: Received non-request message during burst: {:?}", msg);
                }
            }
        }

        assert_eq!(
            requests_received.len(),
            5,
            "Client sent duplicate requests!"
        );

        // ASSERTION 2: Respect Limit
        // Now that we have 5 requests, the 6th REQUEST must not appear.
        // We allow other messages (like KeepAlive), but if we see a Request, it's a violation.
        let result = timeout(Duration::from_millis(50), parse_message(&mut network)).await;

        if let Ok(Ok(extra_msg)) = result {
            if let Message::Request(..) = extra_msg {
                panic!(
                    "Client violated pipeline limit! Sent 6th Request: {:?}",
                    extra_msg
                );
            } else {
                println!("Received innocuous extra message: {:?}", extra_msg);
            }
        }

        // --- Step 3: The Refill (Latency Simulation) ---

        println!("Simulating network latency...");
        // Pick ANY block the client requested and fulfill it.
        let offset_to_fulfill = *requests_received.iter().next().unwrap();

        let block_payload = vec![0xAA; 16384]; // Dummy data
        let piece_msg = Message::Piece(0, offset_to_fulfill, block_payload);
        let piece_bytes = generate_message(piece_msg).expect("Failed to generate piece");
        network
            .write_all(&piece_bytes)
            .await
            .expect("Failed to send piece");

        // ASSERTION 3: Pipeline Maintenance
        println!("Checking for refill request...");
        // Again, ignore non-requests until we get the refill
        let refill_start = std::time::Instant::now();
        let mut got_refill = false;

        while !got_refill {
            if refill_start.elapsed() > Duration::from_millis(500) {
                panic!("Timed out waiting for refill request.");
            }

            let msg = timeout(Duration::from_millis(100), parse_message(&mut network))
                .await
                .expect("Client failed to refill pipeline after receiving data.")
                .expect("Failed to parse refill message");

            if let Message::Request(idx, begin, _) = msg {
                assert_eq!(idx, 0);
                assert!(
                    !requests_received.contains(&begin),
                    "Client re-requested an existing block! Offset: {}",
                    begin
                );
                println!("Success! Client requested new block {} immediately.", begin);
                got_refill = true;
            } else {
                println!(
                    "Note: Received non-request message during refill wait: {:?}",
                    msg
                );
            }
        }
    }

    #[tokio::test]
    async fn test_fragmented_pipeline_saturation() {
        // This test simulates "Stream Saturation" (v2 style).
        // Can we keep the pipe full even if the Manager gives us work in tiny, separate chunks?

        let (mut network, client_cmd_tx, _manager_event_rx) = spawn_test_session().await;

        // --- Setup: Handshake & Bitfield ---
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.unwrap();

        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.unwrap();

        // Consume setup messages
        for _ in 0..5 {
            if let Ok(Ok(Message::Bitfield(_))) =
                timeout(Duration::from_millis(100), parse_message(&mut network)).await
            {
                break;
            }
        }

        // --- The V2 Stress Test ---

        let block_size = 16384;
        let total_torrent_size = block_size * 10; // Enough for 10 pieces

        println!("Queueing 10 separate 'Micro-Downloads' (1 block each)...");
        for i in 0..10 {
            client_cmd_tx
                .send(TorrentCommand::RequestDownload(
                    i as u32,
                    block_size as i64,
                    total_torrent_size as i64,
                ))
                .await
                .expect("Failed to send command");
        }

        // --- ASSERTION 1: Aggregation ---
        println!("Checking for burst of 5 distinct piece requests...");
        let mut requested_pieces = HashSet::new();

        // Loop until we have 5 requests or timeout. Ignore KeepAlives.
        let start = std::time::Instant::now();
        while requested_pieces.len() < 5 {
            if start.elapsed() > Duration::from_millis(500) {
                panic!(
                    "Timed out waiting for 5 requests. Got: {:?}",
                    requested_pieces
                );
            }

            let msg = timeout(Duration::from_millis(100), parse_message(&mut network))
                .await
                .expect("Pipeline Stalled!")
                .expect("Failed to parse");

            match msg {
                Message::Request(index, _, _) => {
                    requested_pieces.insert(index);
                }
                Message::KeepAlive => {
                    println!("Ignored KeepAlive during burst check...");
                }
                _ => panic!("Unexpected message during burst: {:?}", msg),
            }
        }

        assert_eq!(
            requested_pieces.len(),
            5,
            "Client failed to pipeline distinct pieces!"
        );
        println!("Success: Client pipelined pieces {:?}\n", requested_pieces);

        // --- ASSERTION 2: Refill across boundaries ---

        let piece_to_fulfill = *requested_pieces.iter().next().unwrap();
        println!("Fulfilling Piece {}...", piece_to_fulfill);

        let piece_msg = Message::Piece(piece_to_fulfill, 0, vec![0xAA; 16384]);
        let piece_bytes = generate_message(piece_msg).unwrap();
        network.write_all(&piece_bytes).await.unwrap();

        let loop_start = std::time::Instant::now();
        loop {
            if loop_start.elapsed() > Duration::from_millis(500) {
                panic!("Timed out waiting for Refill Request");
            }

            let msg = timeout(Duration::from_millis(200), parse_message(&mut network))
                .await
                .expect("Client stalled after finishing piece.")
                .unwrap();

            match msg {
                Message::Request(index, _, _) => {
                    assert!(
                        !requested_pieces.contains(&index),
                        "Client re-requested finished piece!"
                    );
                    println!(
                        "Success: Client crossed boundary and requested Piece {}",
                        index
                    );
                    break; // Success!
                }
                Message::KeepAlive => {
                    println!("Ignored KeepAlive during refill check...");
                    continue; // Keep looking
                }
                _ => panic!("Expected Request, got {:?}", msg),
            }
        }
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
        let (manager_tx, manager_rx) = mpsc::channel(1000);
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
            // We ignore errors as the test might drop the connection early
            let _ = s.run(client, vec![], Some(vec![])).await;
        });

        (server, cmd_tx, manager_rx)
    }

    #[tokio::test]
    async fn test_token_wallet_respects_limits() {
        // --- TEST 1: CORRECTNESS ---
        // Verify that "Wholesale" batching doesn't accidentally allow speeding.
        // Limit: 1 MB/s. Task: 5 MB. Target Time: ~5.0s.

        let rate_limit = 1_000_000.0;
        let bucket = Arc::new(Mutex::new(TokenBucket::new(rate_limit, rate_limit)));

        let (mut network, client_cmd_tx, mut manager_event_rx) =
            spawn_custom_session(bucket.clone()).await;

        // 1. Handshake Boilerplate
        let mut handshake = vec![0u8; 68];
        network.read_exact(&mut handshake).await.unwrap();
        let mut resp = vec![0u8; 68];
        resp[0] = 19;
        resp[1..20].copy_from_slice(b"BitTorrent protocol");
        network.write_all(&resp).await.unwrap();
        // Consume Bitfield
        let _ = timeout(Duration::from_millis(100), parse_message(&mut network)).await;

        // 2. Setup the transfer
        let total_size = 5 * 1024 * 1024;
        let piece_index = 0;

        client_cmd_tx
            .send(TorrentCommand::RequestDownload(
                piece_index,
                total_size as i64,
                total_size as i64,
            ))
            .await
            .unwrap();

        // 3. Reader Task: Drain 'Requests' so the client doesn't block on a full pipe
        let read_task = tokio::spawn(async move {
            while let Ok(Ok(Message::Request(_, _, _))) =
                timeout(Duration::from_millis(10), parse_message(&mut network)).await
            {
                // Keep reading requests
            }
            network
        });

        // 4. Writer Task: BLAST data at Infinite Speed
        // The Client's "Token Wallet" is the only thing stopping this from finishing instantly.
        let start = std::time::Instant::now();
        let mut network_writer = read_task.await.unwrap();

        let block = vec![0u8; 16384];
        let blocks_to_send = total_size / 16384;

        for i in 0..blocks_to_send {
            let msg = Message::Piece(0, i * 16384, block.clone());
            let bytes = generate_message(msg).unwrap();
            network_writer.write_all(&bytes).await.unwrap();
        }

        // 5. Wait for Client to finish processing
        let mut blocks_received = 0;
        while blocks_received < blocks_to_send {
            if let Some(TorrentCommand::Block(..)) = manager_event_rx.recv().await {
                blocks_received += 1;
            }
        }

        let elapsed = start.elapsed();
        println!(
            "Transferred 5MB in {:.2} seconds (Target: 5.0s)",
            elapsed.as_secs_f64()
        );

        // Assert Correctness:
        // Must not be too fast (Cheating limits)
        assert!(
            elapsed.as_secs_f64() > 4.0,
            "Client ignored rate limit! Batching logic is flawed."
        );
        // Must not be too slow (Lock contention)
        assert!(
            elapsed.as_secs_f64() < 6.0,
            "Client was too slow! Lock contention issue?"
        );
    }

    #[tokio::test]
    async fn test_token_wallet_performance_unthrottled() {
        // --- TEST 2: PERFORMANCE (Overhead Check) ---
        // Verify that "Wholesale" logic handles INFINITY correctly.
        // It should calculate a batch size (clamped to 5MB) and finish instantly.

        let bucket = Arc::new(Mutex::new(TokenBucket::new(f64::INFINITY, f64::INFINITY)));
        let (mut network, client_cmd_tx, mut manager_event_rx) = spawn_custom_session(bucket).await;

        // Handshake
        let mut handshake = vec![0u8; 68];
        network.read_exact(&mut handshake).await.unwrap();
        let mut resp = vec![0u8; 68];
        resp[0] = 19;
        resp[1..20].copy_from_slice(b"BitTorrent protocol");
        network.write_all(&resp).await.unwrap();
        let _ = timeout(Duration::from_millis(100), parse_message(&mut network)).await;

        let total_size = 5 * 1024 * 1024; // 5 MB

        client_cmd_tx
            .send(TorrentCommand::RequestDownload(
                0,
                total_size as i64,
                total_size as i64,
            ))
            .await
            .unwrap();

        // Drain requests
        let read_task = tokio::spawn(async move {
            while let Ok(Ok(Message::Request(_, _, _))) =
                timeout(Duration::from_millis(10), parse_message(&mut network)).await
            {
            }
            network
        });

        let start = std::time::Instant::now();
        let mut network_writer = read_task.await.unwrap();

        // Send data instantly
        let block = vec![0u8; 16384];
        let blocks_to_send = total_size / 16384;

        for i in 0..blocks_to_send {
            let msg = Message::Piece(0, i * 16384, block.clone());
            let bytes = generate_message(msg).unwrap();
            network_writer.write_all(&bytes).await.unwrap();
        }

        // Wait for completion
        let mut blocks_received = 0;
        while blocks_received < blocks_to_send {
            if let Some(TorrentCommand::Block(..)) = manager_event_rx.recv().await {
                blocks_received += 1;
            }
        }

        let elapsed = start.elapsed();
        println!(
            "Transferred 5MB Unthrottled in {:.4} seconds",
            elapsed.as_secs_f64()
        );

        // Assert Performance:
        // 5MB in memory should take milliseconds. If it takes > 0.5s, the batching logic is blocking.
        assert!(
            elapsed.as_secs_f64() < 0.5,
            "Unthrottled performance is degraded!"
        );
    }

    #[tokio::test]
    async fn test_throughput_1000_blocks_batch_cycling() {
        // --- Setup: Infinite Speed ---
        let bucket = Arc::new(Mutex::new(TokenBucket::new(f64::INFINITY, f64::INFINITY)));
        let (mut network, client_cmd_tx, mut manager_event_rx) = spawn_custom_session(bucket).await;

        // --- Handshake ---
        let mut handshake = vec![0u8; 68];
        network.read_exact(&mut handshake).await.unwrap();
        let mut resp = vec![0u8; 68];
        resp[0] = 19;
        resp[1..20].copy_from_slice(b"BitTorrent protocol");
        network.write_all(&resp).await.unwrap();
        let _ = timeout(Duration::from_millis(100), parse_message(&mut network)).await;

        // --- The 1000 Block Load Test ---
        let block_count = 1000;
        let block_size = 16384;
        let total_size = block_count * block_size;

        // 1. Tell client to expect the data
        client_cmd_tx
            .send(TorrentCommand::RequestDownload(
                0,
                total_size as i64,
                total_size as i64,
            ))
            .await
            .unwrap();

        // SPLIT THE NETWORK: This allows us to Read and Write simultaneously
        let (mut peer_read, mut peer_write) = tokio::io::split(network);

        // 2. Drain Requests (Background Task)
        // We just throw away the requests so the client doesn't block on a full send buffer.
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok(n) = peer_read.read(&mut buf).await {
                if n == 0 {
                    break;
                } // EOF
            }
        });

        // 3. BLAST 1000 Blocks (Background Task)
        // By spawning this, we allow the main thread to immediately start checking for results.
        // This prevents the "Channel Full" deadlock/slowdown.
        let block = vec![0u8; block_size as usize];
        let writer_handle = tokio::spawn(async move {
            for i in 0..block_count {
                let msg = Message::Piece(0, i * block_size, block.clone());
                let bytes = generate_message(msg).unwrap();
                // This might block if the client is slow, but that's fine
                // because the main thread is draining the client's output simultaneously.
                peer_write.write_all(&bytes).await.unwrap();
            }
        });

        // 4. Verify All Processed (Main Thread)
        let start = std::time::Instant::now();
        let mut blocks_received = 0;

        while blocks_received < block_count {
            // We expect this to fly. 5s timeout is extremely generous.
            match timeout(Duration::from_secs(5), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Block(..))) => {
                    blocks_received += 1;
                }
                Ok(_) => {} // Ignore other events
                Err(_) => {
                    panic!(
                        "Stalled at block {}/{}! throughput dropped to zero.",
                        blocks_received, block_count
                    );
                }
            }
        }

        // Wait for writer to cleanup (it should be done by now)
        writer_handle.await.unwrap();

        let elapsed = start.elapsed();
        println!(
            "Processed {} blocks (16MB) in {:.4} seconds",
            block_count,
            elapsed.as_secs_f64()
        );

        // Performance Assertion:
        // With concurrency, this should take ~0.05s - 0.20s in Release mode.
        // We set 1.0s as a very safe upper bound for CI/Debug builds.
        assert!(
            elapsed.as_secs_f64() < 3.0,
            "Throughput test failed! System is too slow."
        );
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
                client_cmd_tx.send(TorrentCommand::RequestDownload(
                    0, total_bytes as i64, total_bytes as i64
                )).await.unwrap();

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
