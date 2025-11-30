// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::TorrentCommand;
use reqwest::header::RANGE;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};

// REMOVED: const BLOCK_SIZE = 16384; -> The Manager now controls block size via 'length'

pub async fn web_seed_worker(
    url: String,
    peer_id: String,
    piece_length: u64,
    total_size: u64,
    mut peer_rx: Receiver<TorrentCommand>,
    manager_tx: Sender<TorrentCommand>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let client = reqwest::Client::new();

    // 1. Handshake sequence (Unchanged)
    if manager_tx
        .send(TorrentCommand::SuccessfullyConnected(peer_id.clone()))
        .await
        .is_err()
    {
        return;
    }

    let num_pieces = total_size.div_ceil(piece_length);
    let bitfield_len = num_pieces.div_ceil(8);
    let full_bitfield = vec![255u8; bitfield_len as usize];

    if manager_tx
        .send(TorrentCommand::PeerBitfield(peer_id.clone(), full_bitfield))
        .await
        .is_err()
    {
        return;
    }

    if manager_tx
        .send(TorrentCommand::Unchoke(peer_id.clone()))
        .await
        .is_err()
    {
        return;
    }

    // 2. Main Command Loop
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                break;
            }
            cmd = peer_rx.recv() => {
                match cmd {
                    // UPDATED: Handle specific block request (Micro-command)
                    Some(TorrentCommand::SendRequest { index, begin, length }) => {
                        
                        // Calculate absolute byte range for the HTTP request
                        // Formula: (Piece Index * Piece Size) + Offset within piece
                        let start = (index as u64 * piece_length) + begin as u64;
                        let end = start + length as u64 - 1;
                        let range_header = format!("bytes={}-{}", start, end);

                        let request = client.get(&url).header(RANGE, range_header).send();

                        // Await the Response Header (cancellable)
                        let mut response = match tokio::select! {
                            res = request => res,
                            _ = shutdown_rx.recv() => break,
                        } {
                            Ok(resp) if resp.status().is_success() => resp,
                            _ => {
                                let _ = manager_tx.send(TorrentCommand::Disconnect(peer_id)).await;
                                break;
                            }
                        };

                        // 3. Stream the body
                        // UPDATED: We no longer chop this up. We expect the server to return
                        // exactly 'length' bytes (e.g., 16KB). We gather them and send ONE block back.
                        let mut buffer = Vec::with_capacity(length as usize);

                        loop {
                            let chunk_option = tokio::select! {
                                res = response.chunk() => res,
                                _ = shutdown_rx.recv() => return, 
                            };

                            match chunk_option {
                                Ok(Some(bytes)) => {
                                    buffer.extend_from_slice(&bytes);
                                }
                                Ok(None) => {
                                    // End of stream. Send the accumulated block.
                                    if !buffer.is_empty() {
                                        let _ = manager_tx.send(TorrentCommand::Block(
                                            peer_id.clone(),
                                            index, // piece_index
                                            begin, // block_offset
                                            buffer,
                                        )).await;
                                    }
                                    break; // Finished this request
                                }
                                Err(_) => {
                                    let _ = manager_tx.send(TorrentCommand::Disconnect(peer_id)).await;
                                    return;
                                }
                            }
                        }
                    }
                    Some(TorrentCommand::Disconnect(_)) => break,
                    Some(_) => {} 
                    None => break, 
                }
            }
        }
    }
}
