import hashlib
import os

# --- Configuration ---
FILE_PATH = '/xxx.pdf'  # Ensure this matches your local filename
FILE_SIZE = 8669223
PIECE_INDEX = 132
PIECE_LENGTH = 65536
BLOCK_SIZE = 16384

def calculate_merkle_root():
    # 1. Seek to the start of the last piece
    start_offset = PIECE_INDEX * PIECE_LENGTH
    remaining_size = FILE_SIZE - start_offset
    
    print(f"Checking Piece {PIECE_INDEX}")
    print(f"Piece Data Size: {remaining_size} bytes")
    
    with open(FILE_PATH, 'rb') as f:
        f.seek(start_offset)
        data = f.read(remaining_size)

    # 2. Split data into blocks
    blocks = []
    # Block 1: Full 16KiB
    blocks.append(data[0:BLOCK_SIZE]) 
    # Block 2: Remaining 2,279 bytes
    blocks.append(data[BLOCK_SIZE:])  
    
    print(f"Block 1 size: {len(blocks[0])} bytes")
    print(f"Block 2 size: {len(blocks[1])} bytes")

    # 3. Calculate Leaf Hashes
    # BEP 52: Hash the data blocks
    leaf_hashes = [hashlib.sha256(b).digest() for b in blocks]
    
    # BEP 52: Pad with zero-hashes to fill the binary tree (needs 4 leaves for 64KiB)
    # The spec says "remaining leaf hashes ... are set to zero", meaning 32 bytes of 0x00
    zero_hash = b'\x00' * 32
    while len(leaf_hashes) < 4:
        leaf_hashes.append(zero_hash)
        print(f"Block {len(leaf_hashes)}: [PADDING - Zero Hash]")

    # 4. Build the Tree (Compute Root)
    # Level 1: Hash pairs of leaves
    # Node 0 = Hash(Leaf0 + Leaf1)
    node0 = hashlib.sha256(leaf_hashes[0] + leaf_hashes[1]).digest()
    # Node 1 = Hash(Leaf2 + Leaf3)
    node1 = hashlib.sha256(leaf_hashes[2] + leaf_hashes[3]).digest()
    
    # Level 0: Root = Hash(Node0 + Node1)
    merkle_root = hashlib.sha256(node0 + node1).digest()

    print("-" * 30)
    print(f"Calculated Merkle Root (Hex): {merkle_root.hex()}")
    print("-" * 30)
    print("Compare this hex string with the last 32 bytes of the 'piece layers' in your .torrent file.")

if __name__ == "__main__":
    if os.path.exists(FILE_PATH):
        calculate_merkle_root()
    else:
        print("File not found.")
