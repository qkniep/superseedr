// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use sha2::{Digest, Sha256};

/// Pure function: Verifies a V2 Merkle proof against a target root.
pub fn verify_merkle_proof(
    target_root_hash: &[u8],
    piece_data: &[u8],
    relative_index: u32,
    proof: &[u8],
    hashing_context_len: usize,
) -> bool {
    // 1. Calculate the V2 Root of the data we have using the specific context length
    let calculated_root = compute_v2_piece_root(piece_data, hashing_context_len);
    tracing::info!(
        "Calciulated root {}",
        hex::encode(calculated_root),
    );

    // 2. Local Verification (No Proof)
    if proof.is_empty() {
        let matches = calculated_root.as_slice() == target_root_hash;
        if !matches {
            tracing::info!(
                "V2 Verify Mismatch (Local): Expect {}, Got {}", 
                hex::encode(target_root_hash), hex::encode(calculated_root)
            );
        }
        else {

            tracing::info!(
                "V2 Verify Match (Local): Expect {}, Got {}", 
                hex::encode(target_root_hash), hex::encode(calculated_root)
            );
        }
        return matches;
    }
    else {
        tracing::info!(
            "Empty proof?", 
        );
    }

    // 3. Network Verification (Climb the Tree)
    let mut current_hash = calculated_root;
    let mut current_idx = relative_index; 

    for sibling in proof.chunks(32) {
        let mut hasher = Sha256::new();
        if current_idx % 2 == 0 {
            hasher.update(current_hash);
            hasher.update(sibling);
        } else {
            hasher.update(sibling);
            hasher.update(current_hash);
        }
        current_hash = hasher.finalize().into();
        current_idx /= 2;
    }

    current_hash.as_slice() == target_root_hash
}

/// Pure function: Computes the V2 root of a data block, handling padding logic.
pub fn compute_v2_piece_root(data: &[u8], expected_len: usize) -> [u8; 32] {
    const BLOCK_SIZE: usize = 16_384;
    
    // Determine target leaves (power of two) based on the context length
    let leaf_count = expected_len.div_ceil(BLOCK_SIZE).next_power_of_two();

    tracing::info!(
        "Compute v2 hash data-len {} - expected len {} - leaf_count {}",
        data.len(),
        expected_len,
        leaf_count,
    );

    // 1. Hash 16KB leafs
    let mut layer: Vec<[u8; 32]> = data
        .chunks(BLOCK_SIZE)
        .map(|chunk| {
            Sha256::digest(chunk).into()
        })
        .collect();
    tracing::info!(
        "Leafs data tree len {}",
        layer.len()
    );

    // 2. Pad the layer to the power-of-two leaf count
    // (This handles cases where the file implies more leaves than data provided)
    let empty_hash: [u8; 32] = [0u8; 32];
    while layer.len() < leaf_count {
        layer.push(empty_hash);
    }
    tracing::info!(
        "Leafs node padding tree len {}",
        layer.len()
    );

    // 3. Balanced Binary Reduction
    while layer.len() > 1 {
        layer = layer.chunks(2).map(|pair| {
            let mut hasher = Sha256::new();
            hasher.update(pair[0]);
            // If the tree is balanced (power of 2), pair[1] always exists.
            // If strict bounds checks are needed: if pair.len() > 1 { update(pair[1]) }
            if pair.len() > 1 {
                hasher.update(pair[1]);
            } else {
                // Should not happen if leaf_count is power of two, but safe fallback
            }
            hasher.finalize().into()
        }).collect();
    }
    tracing::info!(
        "Hash tree reduction {}",
        hex::encode(layer[0])
    );
    layer[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_verification_relative_index_parity() {
        // --- SETUP ---
        // We create a file with 2 blocks (32KB total).
        // Block 0: Left Node (Even index)
        // Block 1: Right Node (Odd index)
        let block_size = 16_384;
        let data_0 = vec![0xAA; block_size];
        let data_1 = vec![0xBB; block_size];

        let h0 = Sha256::digest(&data_0);
        let h1 = Sha256::digest(&data_1);

        // Calculate Root: Hash(h0 + h1)
        let mut hasher = Sha256::new();
        hasher.update(h0);
        hasher.update(h1);
        let root: [u8; 32] = hasher.finalize().into();

        // --- SCENARIO: Verify the RIGHT block (Index 1) ---
        // To verify Block 1 (Odd), the proof must contain its left sibling (h0).
        let proof = h0.to_vec();
        
        let is_valid = verify_merkle_proof(
            &root,          // Target Root
            &data_1,         // Our data (Block 1)
            1,               // Relative Index 1 (Odd)
            &proof,          // Proof (Sibling h0)
            16_384           // FIX: Context Len is the block size (16KB), NOT 32KB
        );

        assert!(is_valid, "Merkle verification failed for ODD relative index.");

        // --- SCENARIO: Verify the LEFT block (Index 0) ---
        // To verify Block 0 (Even), the proof must contain its right sibling (h1).
        let proof_0 = h1.to_vec();
        
        let is_valid_0 = verify_merkle_proof(
            &root,
            &data_0,
            0,               // Relative Index 0 (Even)
            &proof_0,
            16_384           // FIX: Context Len is the block size (16KB)
        );

        assert!(is_valid_0, "Merkle verification failed for EVEN relative index.");
    }
}
