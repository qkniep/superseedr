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
    tracing::info!("Calciulated root {}", hex::encode(calculated_root),);

    // 2. Local Verification (No Proof)
    if proof.is_empty() {
        let matches = calculated_root.as_slice() == target_root_hash;
        if !matches {
            tracing::info!(
                "V2 Verify Mismatch (Local): Expect {}, Got {}",
                hex::encode(target_root_hash),
                hex::encode(calculated_root)
            );
        } else {
            tracing::info!(
                "V2 Verify Match (Local): Expect {}, Got {}",
                hex::encode(target_root_hash),
                hex::encode(calculated_root)
            );
        }
        return matches;
    } else {
        tracing::info!("Empty proof?",);
    }

    // 3. Network Verification (Climb the Tree)
    let mut current_hash = calculated_root;
    let mut current_idx = relative_index;

    for sibling in proof.chunks(32) {
        let mut hasher = Sha256::new();
        if current_idx.is_multiple_of(2) {
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
        .map(|chunk| Sha256::digest(chunk).into())
        .collect();
    tracing::info!("Leafs data tree len {}", layer.len());

    // 2. Pad the layer to the power-of-two leaf count
    // (This handles cases where the file implies more leaves than data provided)
    let empty_hash: [u8; 32] = [0u8; 32];
    while layer.len() < leaf_count {
        layer.push(empty_hash);
    }
    tracing::info!("Leafs node padding tree len {}", layer.len());

    // 3. Balanced Binary Reduction
    while layer.len() > 1 {
        layer = layer
            .chunks(2)
            .map(|pair| {
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
            })
            .collect();
    }
    tracing::info!("Hash tree reduction {}", hex::encode(layer[0]));
    layer[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Original Test (Fixed Arguments) ---
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
            &root,   // Target Root
            &data_1, // Our data (Block 1)
            1,       // Relative Index 1 (Odd)
            &proof,  // Proof (Sibling h0)
            16_384, // Context Len must match the BLOCK size, not file size, for the leaf calculation
        );

        assert!(
            is_valid,
            "Merkle verification failed for ODD relative index. Parity logic might be reversed."
        );

        // --- SCENARIO: Verify the LEFT block (Index 0) ---
        // To verify Block 0 (Even), the proof must contain its right sibling (h1).
        let proof_0 = h1.to_vec();

        let is_valid_0 = verify_merkle_proof(
            &root, &data_0, 0, // Relative Index 0 (Even)
            &proof_0, 16_384,
        );

        assert!(
            is_valid_0,
            "Merkle verification failed for EVEN relative index."
        );
    }

    // --- Converted: test_v2_merkle_root_calculation ---
    #[test]
    fn test_v2_merkle_root_calculation() {
        // 1. Create 32KB of deterministic data (2 blocks of 16KB)
        let block_size = 16_384;
        let piece_size = 32_768;
        let mut data = Vec::with_capacity(piece_size);

        // Fill Block 1 with 0xAA, Block 2 with 0xBB
        data.extend_from_slice(&vec![0xAA; block_size]);
        data.extend_from_slice(&vec![0xBB; block_size]);

        // 2. Manually Calculate Expected Root: Hash( Hash(Block1) + Hash(Block2) )
        let hash_1 = Sha256::digest(&data[0..block_size]);
        let hash_2 = Sha256::digest(&data[block_size..piece_size]);

        let mut hasher = Sha256::new();
        hasher.update(hash_1);
        hasher.update(hash_2);
        let expected_root = hasher.finalize();

        // 3. Run the Function Under Test
        let calculated_root = compute_v2_piece_root(&data, data.len());

        // 4. Assert
        assert_eq!(
            calculated_root.as_slice(),
            expected_root.as_slice(),
            "Merkle Root mismatch! The function failed to combine 16KB blocks correctly."
        );
    }

    // --- Converted: test_v2_merkle_root_single_block ---
    #[test]
    fn test_v2_merkle_root_single_block() {
        // 1. Create 16KB data (Single Block)
        let data = vec![0xCC; 16_384];

        // 2. Expected Root is just the SHA256 of the data (Leaf)
        let expected_root = Sha256::digest(&data);

        // 3. Run Function
        let calculated_root = compute_v2_piece_root(&data, data.len());

        // 4. Assert
        assert_eq!(
            calculated_root.as_slice(),
            expected_root.as_slice(),
            "Single block (16KB) should just be hashed directly."
        );
    }

    // --- Converted: verify_tail_padding_fix ---
    #[test]
    fn verify_tail_padding_fix() {
        // Scenario: A file ends 976 bytes into a block.
        // We have a buffer of 976 bytes of data.
        let valid_data_size = 976;
        let valid_data = vec![b'A'; valid_data_size];

        // Rule: Tail blocks are NOT padded with zeros.
        let expected_hash = Sha256::digest(&valid_data);

        // Pass 976 as expected_len so it knows there are no extra tree nodes
        let calculated_root = compute_v2_piece_root(&valid_data, valid_data_size);

        assert_eq!(
            calculated_root.as_slice(),
            expected_hash.as_slice(),
            "V2 Hashing Error: Function padded data incorrectly (Should hash partial data as-is)."
        );
    }

    // --- Converted: test_v2_network_verification_padding_accuracy ---
    #[test]
    fn test_v2_network_verification_padding_accuracy() {
        // 1. SETUP DATA: Simulate a partial tail block (5000 bytes)
        let piece_len: usize = 16384; // The full block size in the system
        let actual_data_len: usize = 5000;
        let raw_data = vec![0xDD; actual_data_len];

        // Calculate CORRECT hash (NO DATA PADDING)
        let correct_leaf_hash = Sha256::digest(&raw_data).to_vec();

        // 3. EXECUTE
        // We simulate the call verify_merkle_proof makes.
        // Note: hashing_context_len passed here is the full block size (16384),
        // but verify_merkle_proof currently uses the *data* length for leaf calculation.
        // This test ensures that behavior holds.
        let is_valid = verify_merkle_proof(
            &correct_leaf_hash,
            &raw_data,
            0,
            &[], // No proof needed for single leaf
            piece_len,
        );

        assert!(
            is_valid,
            "Verification FAILED. Manager likely padded data incorrectly."
        );
    }

    #[test]
    fn test_v2_small_file_less_than_piece_len() {
        // 1. SETUP: Torrent piece_length is 256KB, but file is only 16KB.
        let file_len: usize = 16384;
        let raw_data = vec![0xDD; file_len];

        // In V2, if a file < piece_length, the 'pieces root' is the hash of
        // the file itself (padded to 16KB if it were smaller, but here it is exactly 16KB).
        let expected_file_root = Sha256::digest(&raw_data).to_vec();

        // 3. EXECUTE: Trigger verification for Piece 0
        // FIX: We must pass `file_len` (16KB), NOT `262_144`.
        // If we passed 256KB, the existing code would pad it to 16 blocks.
        // The V2 logic requires that we use the file size as the context for small files.
        let is_valid = verify_merkle_proof(&expected_file_root, &raw_data, 0, &[], file_len);

        assert!(is_valid, "Small file verification failed. Logic likely padded 16KB file to 256KB piece boundary.");
    }

    // --- Converted: test_v2_merkle_parity_regression ---
    #[test]
    fn test_v2_merkle_parity_regression() {
        // 1. SETUP: Create a scenario where Global Index != Relative Index
        // File B starts at Global Piece 1, but it is the FIRST piece of that file (Rel 0).
        let piece_len: usize = 16384;
        let data_b0 = vec![0xAA; piece_len];
        let data_b1 = vec![0xBB; piece_len]; // Neighbor piece to build a tree

        // Calculate Hashes
        let h0 = Sha256::digest(&data_b0).to_vec();
        let h1 = Sha256::digest(&data_b1).to_vec();

        // File Root = Hash(h0 + h1)
        let mut hasher = Sha256::new();
        hasher.update(&h0);
        hasher.update(&h1);
        let file_root = hasher.finalize().to_vec();

        // 2. ACTION: Verify Piece 1 using a Network Proof (h1 is the sibling)
        // Global Index 1 (ODD). Relative Index 0 (EVEN).
        // It SHOULD perform: Hash(Current + Sibling) based on Relative Index.
        let proof = h1; // The sibling needed to climb from h0 to file_root

        let is_valid = verify_merkle_proof(
            &file_root, &data_b0, 0, // Relative Index 0 (Even)
            &proof, piece_len,
        );

        assert!(
            is_valid,
            "Merkle Parity Failed! Piece verified as ODD (Global) instead of EVEN (Relative)."
        );
    }

    // --- Converted: test_v2_small_file_root_mismatch_regression ---
    #[test]
    fn test_v2_small_file_root_mismatch_regression() {
        let actual_file_len: usize = 26_704;
        let data = vec![0xEE; actual_file_len];
        let block_size = 16_384;

        // --- BEP 52 COMPLIANT MANUAL CALCULATION ---
        // 1. Hash the partial data AS-IS (No padding)
        let h0 = Sha256::digest(&data[0..block_size]); // First full 16KB block
        let h1 = Sha256::digest(&data[block_size..]); // Remaining 10,320 bytes

        // 2. Combine them to form the 32KB Piece Root
        let mut hasher = Sha256::new();
        hasher.update(h0);
        hasher.update(h1);
        let expected_file_root: [u8; 32] = hasher.finalize().into();

        // TRIGGER VERIFICATION
        // hashing_context_len 32_768 ensures we build a 2-leaf tree
        let is_valid = verify_merkle_proof(&expected_file_root, &data, 0, &[], 32_768);

        assert!(is_valid, "Verification failed. Manual root calculation must exactly match the 32KB context logic.");
    }

    #[test]
    fn test_compute_root_3_blocks_padding() {
        // Data: 3 full blocks (48KB).
        // Logic should pad to 4 blocks (64KB) with a zero-hash leaf.
        let block_size = 16_384;
        let data = vec![0xCC; block_size * 3];

        // Manual Tree Construction:
        // Leaves: [H(B1), H(B2), H(B3), H(Zero)]
        let h1 = Sha256::digest(&data[0..block_size]);
        let h2 = Sha256::digest(&data[block_size..block_size * 2]);
        let h3 = Sha256::digest(&data[block_size * 2..]);
        let h_zero = [0u8; 32]; // Padding leaf is raw zeros in hash form?
                                // NO. BEP 52 says padding *nodes* are zero hashes.
                                // In `compute_v2_piece_root`: `let empty_hash: [u8; 32] = [0u8; 32];`
                                // So yes, the leaf added is all zeros.

        // Layer 1:
        // Node A = Hash(H1 + H2)
        let mut hasher_a = Sha256::new();
        hasher_a.update(h1);
        hasher_a.update(h2);
        let node_a = hasher_a.finalize();

        // Node B = Hash(H3 + ZeroHash)
        let mut hasher_b = Sha256::new();
        hasher_b.update(h3);
        hasher_b.update(h_zero);
        let node_b = hasher_b.finalize();

        // Root = Hash(Node A + Node B)
        let mut hasher_root = Sha256::new();
        hasher_root.update(node_a);
        hasher_root.update(node_b);
        let expected_root = hasher_root.finalize();

        let actual_root = compute_v2_piece_root(&data, data.len());
        assert_eq!(
            actual_root.as_slice(),
            expected_root.as_slice(),
            "Failed to hash 3-block uneven tree correctly"
        );
    }

    #[test]
    fn test_verify_deep_tree_path() {
        // Scenario: 16 Blocks (256KB). Depth 4.
        // We verify Block 14 (Index 14).
        // Path:
        // 1. Sibling 15 (Right) -> Parent(14,15) is Index 7
        // 2. Sibling 6 (Left)   -> Parent(6,7)   is Index 3
        // 3. Sibling 2 (Left)   -> Parent(2,3)   is Index 1
        // 4. Sibling 0 (Left)   -> Root(0,1)     is Index 0

        // 1. Generate 16 blocks of random hashes
        let leaves: Vec<[u8; 32]> = (0..16)
            .map(|i| {
                let mut h = Sha256::new();
                h.update([i as u8]);
                h.finalize().into()
            })
            .collect();

        // 2. Build Tree Helper (simplified for test)
        fn hash_pair(a: &[u8], b: &[u8]) -> [u8; 32] {
            let mut h = Sha256::new();
            h.update(a);
            h.update(b);
            h.finalize().into()
        }

        let l1: Vec<_> = leaves.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect(); // 8 nodes
        let l2: Vec<_> = l1.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect(); // 4 nodes
        let l3: Vec<_> = l2.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect(); // 2 nodes
        let _root = hash_pair(&l3[0], &l3[1]);

        // 3. Construct Proof for Index 14
        let mut proof = Vec::new();
        proof.extend_from_slice(&leaves[15]); // Sibling of 14
        proof.extend_from_slice(&l1[6]); // Sibling of parent(14,15) -> Index 7's sibling is 6
        proof.extend_from_slice(&l2[1]); // Sibling of Index 3 -> Index 2 (l2[1] is index 1?? No, l2 has indices 0..3. Wait.
                                         // Indices at Layer 2: 0,1,2,3.
                                         // 14/2 = 7. 7/2 = 3. Sibling of 3 is 2. So l2[2]?
                                         // Let's trace carefully:
                                         // L0 Indices: 0..15. Target 14. Sibling 15.
                                         // L1 Indices: 0..7.  Target 7.  Sibling 6.  (Node 6 is l1[6])
                                         // L2 Indices: 0..3.  Target 3.  Sibling 2.  (Node 2 is l2[2])
                                         // L3 Indices: 0..1.  Target 1.  Sibling 0.  (Node 0 is l3[0])

        // Re-do proof construction with correct indices
        let proof_leaves = vec![
            leaves[15], // Neighbor of 14
            l1[6],      // Neighbor of 7
            l2[2],      // Neighbor of 3
            l3[0],      // Neighbor of 1
        ];

        let mut proof_bytes = Vec::new();
        for p in proof_leaves {
            proof_bytes.extend_from_slice(&p);
        }

        // We fake the "data" by just providing its hash as the starting point,
        // since verify_merkle_proof calculates the root of the data first.
        // But verify_merkle_proof takes RAW DATA.
        // So we must provide raw data that hashes to leaves[14].
        // In this test setup, we generated leaves directly from integers, so we can't easily provide matching "data"
        // unless we reverse SHA256 (impossible).

        // FIX: Verify a manually hashed node directly?
        // No, the function signature requires `piece_data`.
        // WORKAROUND: Create actual data for Block 14.
        let block_14_data = vec![0x14; 16384];
        let leaf_14 = Sha256::digest(&block_14_data).into(); // This is the real leaf 14

        // Now rebuild the tree with this ONE real leaf, others can be fake.
        let mut leaves = leaves;
        leaves[14] = leaf_14;

        // Re-hash up
        let l1: Vec<_> = leaves.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect();
        let l2: Vec<_> = l1.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect();
        let l3: Vec<_> = l2.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect();
        let root = hash_pair(&l3[0], &l3[1]);

        // Re-proof
        let proof_bytes = [leaves[15], l1[6], l2[2], l3[0]].concat();

        let is_valid = verify_merkle_proof(
            &root,
            &block_14_data,
            14, // Relative Index 14
            &proof_bytes,
            16384, // Context: Single block
        );

        assert!(is_valid, "Failed to verify deep tree (depth 4) at index 14");
    }

    #[test]
    fn test_verify_fails_on_corruption() {
        let block_size = 16_384;
        let data = vec![0xAA; block_size];
        let root = Sha256::digest(&data);

        // 1. Corrupt Data
        let mut corrupt_data = data.clone();
        corrupt_data[0] = 0xBB; // Flip one byte
        assert!(
            !verify_merkle_proof(&root, &corrupt_data, 0, &[], block_size),
            "Should fail with corrupt data"
        );

        // 2. Corrupt Proof
        // Create a 2-block tree
        let data_sibling = vec![0xBB; block_size];
        let h_sibling = Sha256::digest(&data_sibling);

        let mut hasher = Sha256::new();
        hasher.update(root);
        hasher.update(h_sibling);
        let parent_root = hasher.finalize();

        let mut bad_proof = h_sibling.to_vec();
        bad_proof[0] = bad_proof[0].wrapping_add(1); // Corrupt the proof hash

        assert!(
            !verify_merkle_proof(&parent_root, &data, 0, &bad_proof, block_size),
            "Should fail with corrupt proof"
        );
    }
}
