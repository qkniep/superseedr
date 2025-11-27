// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_manager::state::TorrentStatus;
use std::collections::{HashMap, HashSet};

pub const BLOCK_SIZE: u32 = 16_384; 
pub const V2_HASH_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockAddress {
    pub piece_index: u32,
    pub block_index: u32,
    pub byte_offset: u32,
    pub global_offset: u64,
    pub length: u32,
}

#[derive(Debug, PartialEq)]
pub enum BlockResult {
    /// The block was accepted and state updated. Safe to write to disk.
    Accepted,
    /// The block was already marked done. Discard data.
    Duplicate,
    /// Used for V1 legacy buffering (unchanged)
    V1BlockBuffered,
    /// Used for V1 legacy completion (unchanged)
    V1PieceVerified { piece_index: u32, data: Vec<u8> },
}

#[derive(Default, Debug, Clone)]
pub struct BlockManager {
    // --- STATE ---
    pub block_bitfield: Vec<bool>,
    pub pending_blocks: HashSet<u32>,

    // --- METADATA ---
    pub piece_hashes_v1: Vec<[u8; 20]>,
    // Map of File Index -> SHA-256 Merkle Root
    pub file_merkle_roots: HashMap<usize, [u8; 32]>, 
    
    // --- V1 COMPATIBILITY ---
    pub legacy_buffers: HashMap<u32, Vec<Option<Vec<u8>>>>, 

    // --- GEOMETRY ---
    pub piece_length: u32,
    pub total_length: u64,
    pub total_blocks: u32,
}

impl BlockManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_geometry(
        &mut self, 
        piece_length: u32, 
        total_length: u64, 
        v1_hashes: Vec<[u8; 20]>,
        v2_roots: HashMap<usize, [u8; 32]>,
        validation_complete: bool
    ) {
        self.piece_length = piece_length;
        self.total_length = total_length;
        self.piece_hashes_v1 = v1_hashes;
        self.file_merkle_roots = v2_roots;
        self.total_blocks = (total_length as f64 / BLOCK_SIZE as f64).ceil() as u32;

        self.block_bitfield = vec![validation_complete; self.total_blocks as usize];
    }

    // --- WORK SELECTION ---
    pub fn pick_blocks_for_peer(
        &self,
        peer_bitfield: &[bool],
        count: usize,
        rarest_pieces: &[u32],
    ) -> Vec<BlockAddress> {
        let mut picked = Vec::with_capacity(count);

        for &piece_idx in rarest_pieces {
            if picked.len() >= count { break; }

            if !peer_bitfield.get(piece_idx as usize).unwrap_or(&false) {
                continue;
            }

            let (start_blk, end_blk) = self.get_block_range(piece_idx);

            for global_idx in start_blk..end_blk {
                if picked.len() >= count { break; }

                let already_have = self.block_bitfield.get(global_idx as usize).copied().unwrap_or(true);
                let is_pending = self.pending_blocks.contains(&global_idx);

                if !already_have && !is_pending {
                    picked.push(self.inflate_address(global_idx));
                }
            }
        }
        picked
    }

    pub fn mark_pending(&mut self, global_idx: u32) {
        self.pending_blocks.insert(global_idx);
    }

    pub fn unmark_pending(&mut self, global_idx: u32) {
        self.pending_blocks.remove(&global_idx);
    }

    // --- HELPERS FOR EXTERNAL VERIFIERS ---
    
    /// Returns the expected Merkle Root for a given file index.
    /// The async worker uses this to verify the block *before* calling commit.
    pub fn get_root_for_file(&self, file_index: usize) -> Option<[u8; 32]> {
        self.file_merkle_roots.get(&file_index).copied()
    }

    // --- STATE COMMITMENT ---

    /// Commits a V2 block that has ALREADY been verified by an async worker.
    /// This function is O(1) and safe to call on the main thread.
    pub fn commit_verified_block(&mut self, addr: BlockAddress) -> BlockResult {
        let global_idx = self.flatten_address(addr);

        if global_idx as usize >= self.block_bitfield.len() {
             return BlockResult::Duplicate; // Or error, but treating as dupe is safe
        }

        if self.block_bitfield[global_idx as usize] {
            return BlockResult::Duplicate;
        }

        // Update State
        self.block_bitfield[global_idx as usize] = true;
        self.pending_blocks.remove(&global_idx);

        BlockResult::Accepted
    }

    // --- GEOMETRY HELPERS ---

    fn get_block_range(&self, piece_idx: u32) -> (u32, u32) {
        let piece_len = self.calculate_piece_size(piece_idx);
        let blocks_in_piece = (piece_len as f64 / BLOCK_SIZE as f64).ceil() as u32;
        
        let piece_start_offset = piece_idx as u64 * self.piece_length as u64;
        let start_blk = (piece_start_offset / BLOCK_SIZE as u64) as u32;
        
        (start_blk, start_blk + blocks_in_piece)
    }

    fn calculate_piece_size(&self, piece_idx: u32) -> u32 {
        let offset = piece_idx as u64 * self.piece_length as u64;
        let remaining = self.total_length.saturating_sub(offset);
        std::cmp::min(self.piece_length as u64, remaining) as u32
    }

    pub fn inflate_address(&self, global_idx: u32) -> BlockAddress {
        let global_offset = global_idx as u64 * BLOCK_SIZE as u64;
        let piece_index = (global_offset / self.piece_length as u64) as u32;
        let byte_offset_in_piece = (global_offset % self.piece_length as u64) as u32;
        
        let remaining_len = self.total_length.saturating_sub(global_offset);
        let length = std::cmp::min(BLOCK_SIZE as u64, remaining_len) as u32;

        BlockAddress {
            piece_index,
            block_index: (byte_offset_in_piece / BLOCK_SIZE),
            byte_offset: byte_offset_in_piece,
            global_offset,
            length,
        }
    }

    pub fn flatten_address(&self, addr: BlockAddress) -> u32 {
        (addr.global_offset / BLOCK_SIZE as u64) as u32
    }

    /// V1 HELPER: Check if a full piece is complete.
    /// Used to generate the V1 Bitfield for the handshake.
    pub fn is_piece_complete(&self, piece_index: u32) -> bool {
        let (start, end) = self.get_block_range(piece_index);
        for i in start..end {
            if !self.block_bitfield.get(i as usize).copied().unwrap_or(false) {
                return false;
            }
        }
        true
    }

    /// V1 HELPER: Buffer a block for legacy assembly.
    /// Returns Some(full_piece_bytes) only if the entire piece is ready for SHA-1.
    pub fn handle_v1_block_buffering(&mut self, addr: BlockAddress, data: &[u8]) -> Option<Vec<u8>> {
        let piece_len = self.calculate_piece_size(addr.piece_index);
        let num_blocks = (piece_len as f64 / BLOCK_SIZE as f64).ceil() as usize;

        let buffer_entry = self.legacy_buffers.entry(addr.piece_index).or_insert_with(|| {
             // Vec of Options to track holes. 
             // Ideally use a flat Vec<u8> and a separate 'received' count, 
             // but this is explicit for the example.
             vec![None; num_blocks]
        });

        if (addr.block_index as usize) < buffer_entry.len() {
            buffer_entry[addr.block_index as usize] = Some(data.to_vec());
        }

        // Check if full piece is present
        if buffer_entry.iter().all(|b| b.is_some()) {
             // Assemble
             let mut full_piece = Vec::with_capacity(piece_len as usize);
             for block in buffer_entry.iter() {
                 full_piece.extend_from_slice(block.as_ref().unwrap());
             }
             
             // Clean up buffer (we handed off ownership)
             self.legacy_buffers.remove(&addr.piece_index);
             return Some(full_piece);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_manager() -> BlockManager {
        let mut bm = BlockManager::new();
        // Geometry: Piece=32KB, Total=49252 bytes (~3 pieces worth of blocks?)
        // Actually: 32768 + 16384 + 100 = 49252.
        bm.set_geometry(
            32_768, 
            49_252, 
            vec![[0;20], [0;20]], 
            HashMap::new(), 
            false
        );
        bm
    }

    #[test]
    fn test_commit_verified_block_updates_state() {
        let mut bm = setup_manager();
        let addr = bm.inflate_address(0);
        
        bm.mark_pending(0);
        assert!(bm.pending_blocks.contains(&0));
        assert!(!bm.block_bitfield[0]);

        // Call the commit function (simulating that async verification passed)
        let res = bm.commit_verified_block(addr);

        assert_eq!(res, BlockResult::Accepted);
        assert!(bm.block_bitfield[0]); // Marked done
        assert!(!bm.pending_blocks.contains(&0)); // Pending cleared
    }

    #[test]
    fn test_commit_duplicate_block() {
        let mut bm = setup_manager();
        let addr = bm.inflate_address(0);
        bm.block_bitfield[0] = true; // Already done

        let res = bm.commit_verified_block(addr);
        assert_eq!(res, BlockResult::Duplicate);
    }
}
