
import bencodepy

with open('/xxx.torrent', 'rb') as f:
    torrent_data = bencodepy.decode(f.read())

file_root = bytes.fromhex("e82c4f26ff76d7b43c2e5c82da226e6dcb34a148b4d6ac5232a026a45acfc863")
# Extracting from the piece layers dictionary
layers = torrent_data[b'piece layers'][file_root]
expected_hash = layers[252*32 : 252*32 + 32]

print(f"Piece 252 Expected Hash: {expected_hash.hex()}")
