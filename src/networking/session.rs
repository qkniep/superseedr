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

    writer_rx: Option<Receiver<Message>>,
    writer_tx: Sender<Message>,

    // Tracks blocks IN FLIGHT. Used to credit permits back on receipt/cancel.
    //block_tracker: HashMap<u32, HashSet<BlockInfo>>,
    block_tracker: Arc<Mutex<HashSet<BlockInfo>>>,
    
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

// session.rs

impl PeerSession {
    pub fn new(params: PeerSessionParameters) -> Self {
        // Increase channel size to buffer the Manager's requests
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
        
        // FIX 1: Use take() to move writer_rx out without breaking 'self'
        let writer_rx = self.writer_rx.take().ok_or("Writer RX missing")?;
        
        let writer_handle = tokio::spawn(writer_task(
            stream_write_half,
            writer_rx,
            error_tx,
            global_ul_bucket_clone,
            writer_shutdown_rx,
        ));
        let _writer_abort_guard = AbortOnDrop(writer_handle);

        // ... (Handshake logic remains the same) ...
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

        // FIX 2: Create local variables to satisfy the borrow checker
        let mut keep_alive_timer = tokio::time::interval(Duration::from_secs(60));
        let inactivity_timeout = tokio::time::sleep(Duration::from_secs(120));
        tokio::pin!(inactivity_timeout);

        let mut shutdown_rx = self.shutdown_tx.subscribe(); // Created ONCE here
        let manager_tx = self.torrent_manager_tx.clone(); // Clone for reserve()

        let _result: Result<(), Box<dyn StdError + Send + Sync>> = 'session: loop {
            const READ_TIMEOUT: Duration = Duration::from_secs(120);

            tokio::select! {
                _ = &mut inactivity_timeout => break 'session Err("Timeout".into()),
                _ = keep_alive_timer.tick() => { let _ = self.writer_tx.try_send(Message::KeepAlive); },
                
                Ok(message_from_peer) = timeout(READ_TIMEOUT, parse_message(&mut stream_read_half)) => {
                    match message_from_peer {
                        Ok(msg) => {
                            inactivity_timeout.as_mut().reset(Instant::now() + Duration::from_secs(120));
                            match msg {
                                Message::Piece(index, begin, data) => {

                                    let block_len = data.len() as u32;
                                    let block_info = BlockInfo {
                                        piece_index: index,
                                        offset: begin,
                                        length: block_len,
                                    };
                                    if self.block_tracker.lock().await.remove(&block_info) {
                                        self.block_request_limit_semaphore.add_permits(1);
                                    }

                                    let cmd = TorrentCommand::Block(self.peer_ip_port.clone(), index, begin, data);
                                    
                                    // INNER BACKPRESSURE LOOP
                                    loop {
                                        tokio::select! {
                                            // FIX 3: Use local 'manager_tx' clone to avoid locking 'self'
                                            permit_res = manager_tx.reserve() => {
                                                match permit_res {
                                                    Ok(permit) => {
                                                        permit.send(cmd);
                                                        break;
                                                    }
                                                    Err(_) => break 'session Err("Manager Closed".into()),
                                                }
                                            }
                                            Some(cmd) = self.torrent_manager_rx.recv() => {
                                                if !self.process_manager_command(cmd)? { break 'session Ok(()); }
                                            }
                                            _ = shutdown_rx.recv() => break 'session Ok(()),
                                        }
                                    }
                                }
                                // ... (Rest of Message handlers same as previous code) ...
                                Message::Choke => {
                                    self.block_tracker.lock().await.clear();
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
                        }
                        Err(e) => break 'session Err(e.into()),
                    }
                },
                
                Some(cmd) = self.torrent_manager_rx.recv() => {
                    if !self.process_manager_command(cmd)? { break 'session Ok(()); }
                },
                
                writer_res = &mut error_rx => {
                    break 'session Err(writer_res.unwrap_or_else(|_| "Writer panicked".into()));
                },
                
                // FIX 4: Use the variable created outside
                _ = shutdown_rx.recv() => break 'session Ok(()),
            }
        };

        if !self.block_request_joinset.is_empty() {
            while self.block_request_joinset.join_next().await.is_some() {}
        }

        Ok(())
    }

    /// Handles incoming commands from the Manager.
    /// Returns `Ok(true)` to continue, `Ok(false)` to stop the session gracefully.
    fn process_manager_command(
        &mut self,
        command: TorrentCommand,
    ) -> Result<bool, Box<dyn StdError + Send + Sync>> {
        match command {
            // --- CRITICAL CONTROL ---
            TorrentCommand::Disconnect(_) => return Ok(false),

            // Use the exact enum variants defined in your `command.rs`.
            // If PeerChoke doesn't exist, map standard Choke to the writer.
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
            TorrentCommand::Cancel(index, begin, len) => {
                let _ = self
                    .writer_tx
                    .try_send(Message::Cancel(index, begin, len));

                if let Ok(mut buf) = self.cancellation_buffer.try_lock() {
                    if buf.len() >= 50 {
                        buf.pop_front();
                    }
                    buf.push_back((index, begin));
                }
            }

            // --- OUTGOING REQUESTS (FIXED) ---
            TorrentCommand::SendRequest {
                index,
                begin,
                length,
            } => {
                let writer = self.writer_tx.clone();
                let sem = self.block_request_limit_semaphore.clone();
                
                let tracker = self.block_tracker.clone(); 
                
                let cancel_buf = self.cancellation_buffer.clone();
                let mut shutdown = self.shutdown_tx.subscribe();

                self.block_request_joinset.spawn(async move {
                    // 1. Wait for a slot (Throttle)
                    let permit = tokio::select! {
                        Ok(p) = sem.acquire_owned() => p,
                        _ = shutdown.recv() => return,
                    };

                    // 2. Check for cancellation
                    {
                        let buf = cancel_buf.lock().await;
                        if buf.contains(&(index, begin)) {
                            return;
                        }
                    }

                    // 3. Send Request
                    if writer
                        .send(Message::Request(index, begin, length))
                        .await
                        .is_ok()
                    {
                        // 4. REGISTER THE BLOCK (The Missing Link)
                        // We must add it to the tracker so Message::Piece can remove it later
                        // and refill the semaphore.
                        {
                            let mut t = tracker.lock().await;
                            t.insert(BlockInfo {
                                piece_index: index,
                                offset: begin,
                                length,
                            });
                        }

                        // 5. Release the permit from the scope, effectively transferring 
                        // ownership of the "slot" to the tracker/Message::Piece handler.
                        permit.forget();
                    }
                });
            }

            // --- SEEDING ---
            TorrentCommand::Upload(index, begin, data) => {
                let _ = self
                    .writer_tx
                    .try_send(Message::Piece(index, begin, data));
            }

            // --- METADATA / PEX ---
            TorrentCommand::PeerBitfield(_, bf) => {
                let _ = self.writer_tx.try_send(Message::Bitfield(bf));
            }

            #[cfg(feature = "pex")]
            TorrentCommand::SendPexPeers(peers) => {
                self.handle_pex(peers);
            }

            // --- HANDSHAKE / OTHER ---
            TorrentCommand::Have(_, idx) => {
                let _ = self.writer_tx.try_send(Message::Have(idx));
            }

            // --- SAFETY CATCH ---
            // 'Block' command is OUTGOING only. If we receive it, it's a bug in Manager.
            TorrentCommand::Block(..) => {
                event!(Level::ERROR, "Logic Error: Session received Block command!");
            }

            // --- IGNORE / WILDCARD ---
            // Handle events meant for Manager but routed here by mistake, or internal events
            // like SuccessfullyConnected that don't require Session action.
            _ => {
                // event!(Level::TRACE, "Ignored manager command in session: {:?}", command);
            }
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
                            match serde_bencode::to_bytes(&request) {
                                Ok(payload_bytes) => {
                                    let _ = self.writer_tx.try_send(Message::Extended(
                                        ClientExtendedId::UtMetadata.id(),
                                        payload_bytes,
                                    ));
                                }
                                Err(e) => {
                                    event!(
                                        Level::ERROR,
                                        "Failed to serialize metadata request: {}",
                                        e
                                    );
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
                        let _ = self.torrent_manager_tx.try_send(
                            TorrentCommand::AddPexPeers(self.peer_ip_port.clone(), new_peers),
                        );
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
                            let dht_info_result: Result<Info, _> =
                                serde_bencode::from_bytes(&self.peer_torrent_metadata_pieces[..]);

                            match dht_info_result {
                                Ok(dht_info) => {
                                    let _ = self.torrent_manager_tx.try_send(
                                        TorrentCommand::DhtTorrent(
                                            Torrent {
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
                                            },
                                            torrent_metadata_len,
                                        ),
                                    );
                                }
                                Err(e) => {
                                    event!(
                                        Level::WARN,
                                        "Failed to decode torrent metadata from peer: {}",
                                        e
                                    );
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
                                    let _ = self.writer_tx.try_send(Message::Extended(
                                        ClientExtendedId::UtMetadata.id(),
                                        payload_bytes,
                                    ));
                                }
                                Err(e) => {
                                    event!(
                                        Level::ERROR,
                                        "Failed to serialize metadata request: {}",
                                        e
                                    );
                                }
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
    use crate::networking::protocol::{generate_message, parse_message, Message};
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
        let (client_socket, mock_peer_socket) = duplex(64 * 1024);
        let infinite_bucket = Arc::new(Mutex::new(TokenBucket::new(f64::INFINITY, f64::INFINITY)));
        let (manager_tx, manager_rx) = mpsc::channel(100);
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
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
        // Send 5 requests.
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

        // Send 5 separate commands for 5 separate pieces
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
        while blocks_requested < WINDOW_SIZE {
            client_cmd_tx.send(TorrentCommand::SendRequest {
                index: blocks_requested, begin: 0, length: BLOCK_SIZE as u32
            }).await.unwrap();
            blocks_requested += 1;
        }

        // Process loop
        while blocks_received < TOTAL_BLOCKS {
            match timeout(Duration::from_secs(5), manager_event_rx.recv()).await {
                Ok(Some(TorrentCommand::Block(..))) => {
                    blocks_received += 1;
                    if blocks_requested < TOTAL_BLOCKS {
                        client_cmd_tx.send(TorrentCommand::SendRequest {
                            index: blocks_requested, begin: 0, length: BLOCK_SIZE as u32
                        }).await.unwrap();
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
}

