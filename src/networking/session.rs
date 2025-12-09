// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_file::Info;
use crate::torrent_file::Torrent;

use super::protocol::{
     writer_task, reader_task, BlockInfo, ClientExtendedId,
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

const PEER_BLOCK_IN_FLIGHT_LIMIT: usize = 16;

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
    
    block_upload_limit_semaphore: Arc<Semaphore>,

    peer_extended_id_mappings: HashMap<String, u8>,
    peer_extended_handshake_payload: Option<ExtendedHandshakePayload>,
    peer_torrent_metadata_piece_count: usize,
    peer_torrent_metadata_pieces: Vec<u8>,

    global_dl_bucket: Arc<TokenBucket>,
    global_ul_bucket: Arc<TokenBucket>,

    shutdown_tx: broadcast::Sender<()>,
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
            block_upload_limit_semaphore: Arc::new(Semaphore::new(PEER_BLOCK_IN_FLIGHT_LIMIT)),
            peer_extended_id_mappings: HashMap::new(),
            peer_extended_handshake_payload: None,
            peer_torrent_metadata_piece_count: 0,
            peer_torrent_metadata_pieces: Vec::new(),
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
        let _ = self.torrent_manager_tx.try_send(TorrentCommand::PeerId(self.peer_ip_port.clone(), peer_id));

        if (handshake_response[25] & 0x10) != 0 {
            let meta_len = self.torrent_metadata_length.clone();
            let _ = self.writer_tx.try_send(Message::ExtendedHandshake(meta_len));
        }

        if let Some(bitfield) = current_bitfield {
            self.peer_session_established = true;
            let _ = self.writer_tx.try_send(Message::Bitfield(bitfield));
            let _ = self.torrent_manager_tx.try_send(TorrentCommand::SuccessfullyConnected(self.peer_ip_port.clone()));
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

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let manager_tx = self.torrent_manager_tx.clone();

        let _result: Result<(), Box<dyn StdError + Send + Sync>> = 'session: loop {
            tokio::select! {
                // Timeout Check
                _ = &mut inactivity_timeout => break 'session Err("Timeout".into()),
                
                // KeepAlive
                _ = keep_alive_timer.tick() => { let _ = self.writer_tx.try_send(Message::KeepAlive); },
                
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
                                self.block_request_limit_semaphore.add_permits(1);
                                
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
                            // Refill permits
                            let max = PEER_BLOCK_IN_FLIGHT_LIMIT;
                            let current = self.block_request_limit_semaphore.available_permits();
                            if current < max { self.block_request_limit_semaphore.add_permits(max - current); }
                            let _ = self.torrent_manager_tx.try_send(TorrentCommand::Choke(self.peer_ip_port.clone()));
                        }
                        Message::Unchoke => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::Unchoke(self.peer_ip_port.clone())); }
                        Message::Interested => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::PeerInterested(self.peer_ip_port.clone())); }
                        Message::NotInterested => {}
                        Message::Have(idx) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::Have(self.peer_ip_port.clone(), idx)); }
                        Message::Bitfield(bf) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::PeerBitfield(self.peer_ip_port.clone(), bf)); }
                        Message::Request(i, b, l) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::RequestUpload(self.peer_ip_port.clone(), i, b, l)); }
                        Message::Cancel(i, b, l) => { let _ = self.torrent_manager_tx.try_send(TorrentCommand::CancelUpload(self.peer_ip_port.clone(), i, b, l)); }
                        Message::Extended(id, p) => { self.handle_extended_message(id, p).await?; }
                        Message::KeepAlive => {}
                        Message::Port(_) => {}
                        Message::Handshake(..) => {}
                        Message::ExtendedHandshake(_) => {}
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
                
                // Capture context for reaper
                let manager_tx = self.torrent_manager_tx.clone();
                let peer_ip = self.peer_ip_port.clone();

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

                            let info = BlockInfo { piece_index: index, offset: begin, length };

                            {
                                let mut t = tracker.lock().await;
                                t.insert(info.clone());
                            }

                            if writer.send(Message::Request(index, begin, length)).await.is_ok() {
                                
                                // --- ZOMBIE REAPER ---
                                let sem_clone = sem.clone();
                                let tracker_clone = tracker.clone();
                                let info_clone = info.clone();
                                let manager_tx_clone = manager_tx.clone();
                                let peer_ip_clone = peer_ip.clone();

                                tokio::spawn(async move {
                                    // 20s Timeout: Enough for RTT + Queue, but kills dead connections
                                    tokio::time::sleep(Duration::from_secs(20)).await;

                                    let mut t = tracker_clone.lock().await;
                                    if t.remove(&info_clone) {
                                        // 1. Reclaim Permit
                                        sem_clone.add_permits(1);
                                        tracing::event!(Level::DEBUG, "ZOMBIE REAPER: Peer {} timed out on block {}@{}. Disconnecting.", peer_ip_clone, info_clone.piece_index, info_clone.offset);
                                        // 2. Kill Session to release blocks back to Manager
                                        let _ = manager_tx_clone.send(TorrentCommand::Disconnect(peer_ip_clone)).await;
                                    }
                                });

                                permit.forget();
                            } else {
                                { let mut t = tracker.lock().await; t.remove(&info); }
                                break;
                            }
                        }
                    }
                });
            }

            TorrentCommand::BulkCancel(cancels) => {
                for (index, begin, len) in &cancels {
                    let _ = self.writer_tx.try_send(Message::Cancel(*index, *begin, *len));
                }

                let tracker = self.block_tracker.clone();
                let sem = self.block_request_limit_semaphore.clone();
                
                tokio::spawn(async move {
                    let mut tracker_guard = tracker.lock().await;
                    let mut permits_to_add = 0;
                    for (index, begin, length) in cancels {
                        let info = BlockInfo { piece_index: index, offset: begin, length };
                        if tracker_guard.remove(&info) { permits_to_add += 1; }
                    }
                    if permits_to_add > 0 { sem.add_permits(permits_to_add); }
                });
            }

            TorrentCommand::Upload(index, begin, data) => {
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
            _ => {}
        }
        Ok(true)
    }

    #[cfg(feature = "pex")]
    fn handle_pex(&self, peers_list: Vec<String>) {
        if let Some(pex_id) = self.peer_extended_id_mappings.get(ClientExtendedId::UtPex.as_str()).copied() {
            let added: Vec<u8> = peers_list.iter()
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
                let msg = PexMessage { added, ..Default::default() };
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
            if let Ok(handshake_data) = serde_bencode::from_bytes::<ExtendedHandshakePayload>(&payload) {
                self.peer_extended_id_mappings = handshake_data.m.clone();
                if !handshake_data.m.is_empty() {
                    self.peer_extended_handshake_payload = Some(handshake_data.clone());
                    if !self.peer_session_established {
                        if let Some(_torrent_metadata_len) = handshake_data.metadata_size {
                            let request = MetadataMessage { msg_type: 0, piece: 0, total_size: None };
                            if let Ok(payload_bytes) = serde_bencode::to_bytes(&request) {
                                let _ = self.writer_tx.try_send(Message::Extended(ClientExtendedId::UtMetadata.id(), payload_bytes));
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
                        let _ = self.torrent_manager_tx.try_send(TorrentCommand::AddPexPeers(self.peer_ip_port.clone(), new_peers));
                    }
                }
            }
        }

        if extended_id == ClientExtendedId::UtMetadata.id() && !self.peer_session_established {
            if let Some(ref handshake_data) = self.peer_extended_handshake_payload {
                if let Some(torrent_metadata_len) = handshake_data.metadata_size {
                    let torrent_metadata_len_usize = torrent_metadata_len as usize;
                    let current_offset = self.peer_torrent_metadata_piece_count * 16384;
                    let expected_data_len = std::cmp::min(16384, torrent_metadata_len_usize.saturating_sub(current_offset));
                    
                    if payload.len() >= expected_data_len {
                        let header_len = payload.len() - expected_data_len;
                        let metadata_binary = &payload[header_len..];
                        self.peer_torrent_metadata_pieces.extend(metadata_binary);

                        if torrent_metadata_len_usize == self.peer_torrent_metadata_pieces.len() {
                            if let Ok(dht_info) = serde_bencode::from_bytes(&self.peer_torrent_metadata_pieces[..]) {
                                let _ = self.torrent_manager_tx.try_send(TorrentCommand::DhtTorrent(
                                    Torrent {
                                        info_dict_bencode: self.peer_torrent_metadata_pieces.clone(),
                                        info: dht_info,
                                        announce: None, announce_list: None, url_list: None, creation_date: None, comment: None, created_by: None, encoding: None,
                                    },
                                    torrent_metadata_len,
                                ));
                            }
                        } else {
                            self.peer_torrent_metadata_piece_count += 1;
                            let request = MetadataMessage { msg_type: 0, piece: self.peer_torrent_metadata_piece_count, total_size: None };
                            if let Ok(payload_bytes) = serde_bencode::to_bytes(&request) {
                                let _ = self.writer_tx.try_send(Message::Extended(ClientExtendedId::UtMetadata.id(), payload_bytes));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::protocol::{generate_message, Message};
    use std::collections::HashSet;
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

    // --- Helper: Setup a Session with a Virtual Pipe ---
    async fn spawn_test_session() -> (
        tokio::io::DuplexStream,        // The "Network" side (Mock Peer)
        mpsc::Sender<TorrentCommand>,   // Channel to command the Client (Manager -> Session)
        mpsc::Receiver<TorrentCommand>, // Channel to receive events from Client (Session -> Manager)
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

        tokio::spawn(async move {
            let session = PeerSession::new(params);
            if let Err(e) = session.run(client_socket, vec![], Some(vec![])).await {
                // Log but don't panic, allows tests to clean up
                eprintln!("Test Session ended: {:?}", e);
            }
        });

        (mock_peer_socket, cmd_tx, manager_rx)
    }

    #[tokio::test]
    async fn test_pipeline_saturation_with_virtual_time() {
        let (mut network, client_cmd_tx, _manager_event_rx) = spawn_test_session().await;

        // --- Step 1: Handshake ---
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.expect("Failed to read client handshake");

        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        network.write_all(&response).await.expect("Failed to write handshake");

        // Consume Initial Messages (Bitfield, Extended Handshake, etc.)
        // We read until we stop getting messages for a short duration
        let start_drain = Instant::now();
        while start_drain.elapsed() < Duration::from_millis(500) {
            if let Ok(Ok(_)) = timeout(Duration::from_millis(50), parse_message(&mut network)).await {
                continue; 
            } else {
                break; // No more immediate messages
            }
        }

        // --- Step 2: The Saturation Test ---
        // Send 5 requests in a single bulk command.
        let requests: Vec<_> = (0..5)
            .map(|i| (0, i * 16384, 16384))
            .collect();
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
                Ok(Ok(_)) => {}, // Ignore KeepAlives or late Metadata messages
                Ok(Err(_)) => break, // Socket closed
                Err(_) => {}, // Timeout, keep retrying until overall_timeout
            }
        }
        
        assert_eq!(requests_received.len(), 5, "Failed to receive all 5 requests in burst. Got: {:?}", requests_received);
    }

    #[tokio::test]
    async fn test_fragmented_pipeline_saturation() {
        let (mut network, client_cmd_tx, _manager_event_rx) = spawn_test_session().await;

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
            if let Ok(Ok(_)) = timeout(Duration::from_millis(50), parse_message(&mut network)).await { continue; } else { break; }
        }

        // Send 5 separate commands for 5 separate pieces in a single bulk command
        let requests: Vec<_> = (0..5)
            .map(|i| (i as u32, 0, 16384))
            .collect();
        client_cmd_tx
            .send(TorrentCommand::BulkRequest(requests))
            .await
            .expect("Failed to send bulk command");

        let mut requested_pieces = HashSet::new();
        let start = Instant::now();
        
        while requested_pieces.len() < 5 {
            if start.elapsed() > Duration::from_secs(5) { break; }
            
            match timeout(Duration::from_secs(1), parse_message(&mut network)).await {
                Ok(Ok(Message::Request(idx, _, _))) => {
                    requested_pieces.insert(idx);
                },
                _ => {} // Retry
            }
        }
        
        assert_eq!(requested_pieces.len(), 5, "Failed to receive all 5 fragmented requests. Got: {:?}", requested_pieces);
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
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        
        tokio::spawn(async move {
            let mut am_choking = true;

            loop {
                match timeout(Duration::from_secs(5), parse_message(&mut peer_read)).await {
                    Ok(Ok(msg)) => match msg {
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
                                let piece = generate_message(Message::Piece(index, begin, data)).unwrap();
                                if peer_write.write_all(&piece).await.is_err() { break; }
                            }
                        }
                        _ => {} 
                    },
                    _ => break,
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

        client_cmd_tx.send(TorrentCommand::ClientInterested).await.unwrap();

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
                        client_cmd_tx.send(TorrentCommand::BulkRequest(vec![(
                            blocks_requested, 0, BLOCK_SIZE as u32
                        )])).await.unwrap();
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
        println!("Success: {:.2} MB in {:.2?} ({:.2} MB/s)", total_mb, elapsed, total_mb / elapsed.as_secs_f64());
    }

    #[tokio::test]
    async fn test_bug_repro_unsolicited_forwarding() {
        let (mut network, _client_cmd_tx, mut manager_rx) = spawn_test_session().await;

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
            if let Ok(Ok(_)) = timeout(Duration::from_millis(10), parse_message(&mut network)).await { continue; } else { break; }
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
        let (mut network, client_cmd_tx, mut manager_event_rx, session_handle) = spawn_debug_session().await;

        // 2. Handshake
        let mut handshake_buf = vec![0u8; 68];
        network.read_exact(&mut handshake_buf).await.expect("Handshake read failed");
        let mut response = vec![0u8; 68];
        response[0] = 19;
        response[1..20].copy_from_slice(b"BitTorrent protocol");
        response[20..28].copy_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]); 
        network.write_all(&response).await.expect("Handshake write failed");

        // 3. Mock Peer (High Perf)
        let (mut peer_read, mut peer_write) = tokio::io::split(network);
        tokio::spawn(async move {
            let mut am_choking = true;
            let dummy_data = vec![0xAA; BLOCK_SIZE];
            loop {
                // 30s timeout to prevent starvation
                match timeout(Duration::from_secs(30), parse_message(&mut peer_read)).await {
                    Ok(Ok(msg)) => match msg {
                        Message::Interested => {
                            if am_choking {
                                let unchoke = generate_message(Message::Unchoke).unwrap();
                                if peer_write.write_all(&unchoke).await.is_err() { break; }
                                am_choking = false;
                            }
                        }
                        Message::Request(index, begin, _len) => {
                            if !am_choking {
                                let piece_msg = generate_message(Message::Piece(index, begin, dummy_data.clone())).unwrap();
                                if peer_write.write_all(&piece_msg).await.is_err() { break; }
                            }
                        }
                        _ => {}
                    },
                    _ => break, 
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

        client_cmd_tx.send(TorrentCommand::ClientInterested).await.unwrap();

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
            .map(|i| { blocks_requested += 1; (i, 0, BLOCK_SIZE as u32) })
            .collect();
        
        client_cmd_tx.send(TorrentCommand::BulkRequest(initial_batch)).await.expect("Failed to send initial batch");

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
        println!("DONE: {:.2} MB in {:.2?} ({:.2} MB/s)", mb, elapsed, mb / elapsed.as_secs_f64());
    }
}
