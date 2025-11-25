// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::TorrentCommand;
use reqwest::header::RANGE;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};

const BLOCK_SIZE: usize = 16384; // Standard 16KB block size

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

    // 1. Handshake sequence
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
                    Some(TorrentCommand::RequestDownload(piece_index, length, _)) => {
                        // Calculate absolute byte range for the HTTP request
                        let start = piece_index as u64 * piece_length;
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

                        // 3. Stream the body and chunk it into blocks
                        let mut buffer = Vec::with_capacity(BLOCK_SIZE * 2);
                        let mut current_block_offset = 0u32;

                        loop {
                            // Await the next network chunk (cancellable)
                            let chunk_option = tokio::select! {
                                res = response.chunk() => res,
                                _ = shutdown_rx.recv() => {
                                    return; // Exit function immediately on shutdown
                                }
                            };

                            match chunk_option {
                                Ok(Some(bytes)) => {
                                    buffer.extend_from_slice(&bytes);

                                    // While we have enough data for a full block, slice it off and send it
                                    while buffer.len() >= BLOCK_SIZE {
                                        let data: Vec<u8> = buffer.drain(..BLOCK_SIZE).collect();

                                        let send_result = manager_tx.send(TorrentCommand::Block(
                                            peer_id.clone(),
                                            piece_index,
                                            current_block_offset,
                                            data,
                                        )).await;

                                        if send_result.is_err() {
                                            return; // Manager died
                                        }

                                        current_block_offset += BLOCK_SIZE as u32;
                                    }
                                }
                                Ok(None) => {
                                    // End of stream. Send any remaining bytes (tail of the piece).
                                    if !buffer.is_empty() {
                                        let _ = manager_tx.send(TorrentCommand::Block(
                                            peer_id.clone(),
                                            piece_index,
                                            current_block_offset,
                                            buffer,
                                        )).await;
                                    }
                                    break; // Finished this piece
                                }
                                Err(_) => {
                                    // Network error mid-stream
                                    let _ = manager_tx.send(TorrentCommand::Disconnect(peer_id)).await;
                                    return;
                                }
                            }
                        }
                    }
                    Some(TorrentCommand::Disconnect(_)) => break,
                    Some(_) => {} // Ignore Choke, Interested, etc.
                    None => break, // Channel closed
                }
            }
        }
    }
}
