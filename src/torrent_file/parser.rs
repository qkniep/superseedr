// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::torrent_file::Torrent;
use serde_bencode::de;
use serde_bencode::value::Value;

use std::fmt;

#[derive(Debug)]
pub enum ParseError {
    Bencode(serde_bencode::Error),
    MissingInfoDict,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            // For the Bencode variant, we now use the contained error `e`
            ParseError::Bencode(e) => write!(f, "Bencode parsing error: {}", e),
            ParseError::MissingInfoDict => write!(f, "Missing 'info' dictionary in torrent file"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<serde_bencode::Error> for ParseError {
    fn from(e: serde_bencode::Error) -> Self {
        ParseError::Bencode(e)
    }
}

pub fn from_bytes(bencode_data: &[u8]) -> Result<Torrent, ParseError> {
    // 1. First, deserialize the data into a generic Bencode Value structure.
    //    This allows us to inspect the raw data before converting to our final struct.
    let generic_bencode: Value = de::from_bytes(bencode_data)?;

    // 2. Extract the raw 'info' dictionary value.
    let info_dict_value = if let Value::Dict(mut top_level_dict) = generic_bencode.clone() {
        top_level_dict
            .remove("info".as_bytes())
            .ok_or(ParseError::MissingInfoDict)?
    } else {
        return Err(ParseError::MissingInfoDict);
    };

    // 3. Re-encode just the 'info' dictionary to get the bytes needed for the info_hash.
    let info_dict_bencode = serde_bencode::to_bytes(&info_dict_value)?;

    // 4. Deserialize the original data AGAIN, but this time into our strongly-typed Torrent struct.
    //    Serde is fast, so this second pass is not a major performance issue.
    let mut torrent: Torrent = de::from_bytes(bencode_data)?;

    // If 'length' is 0 (typical for Pure V2), calculate it from the file tree
    // so the rest of the application sees a valid size immediately.
    if torrent.info.length == 0 {
        torrent.info.length = torrent.info.total_length();
    }

    // 5. Manually set the `info_dict_bencode` field we created.
    torrent.info_dict_bencode = info_dict_bencode;

    Ok(torrent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent_file::{Info, Torrent};
    use serde_bencode::value::Value;
    use std::collections::HashMap; // Changed from BTreeMap

    #[test]
    fn test_parse_bittorrent_v2_hybrid_structure() {
        // --- 1. Construct Manual v2 Data Structures ---
        let root_hash_1 = vec![0xAA; 32];
        let root_hash_2 = vec![0xBB; 32];

        // Use HashMap instead of BTreeMap
        let mut file_a_metadata = HashMap::new();
        file_a_metadata.insert(
            "pieces root".as_bytes().to_vec(),
            Value::Bytes(root_hash_1.clone()),
        );
        file_a_metadata.insert("length".as_bytes().to_vec(), Value::Int(1000));
        
        let mut leaf_node_a = HashMap::new();
        leaf_node_a.insert(vec![], Value::Dict(file_a_metadata));

        let mut file_b_metadata = HashMap::new();
        file_b_metadata.insert(
            "pieces root".as_bytes().to_vec(),
            Value::Bytes(root_hash_2.clone()),
        );
        file_b_metadata.insert("length".as_bytes().to_vec(), Value::Int(2000));
        
        let mut leaf_node_b = HashMap::new();
        leaf_node_b.insert(vec![], Value::Dict(file_b_metadata));

        let mut folder_contents = HashMap::new();
        folder_contents.insert("file_a.txt".as_bytes().to_vec(), Value::Dict(leaf_node_a));

        let mut tree_root = HashMap::new();
        tree_root.insert(
            "folder".as_bytes().to_vec(),
            Value::Dict(folder_contents),
        );
        tree_root.insert(
            "file_b.txt".as_bytes().to_vec(),
            Value::Dict(leaf_node_b),
        );

        let mut layers = HashMap::new();
        layers.insert(root_hash_1.clone(), Value::Bytes(vec![0x11; 32]));
        layers.insert(root_hash_2.clone(), Value::Bytes(vec![0x22; 32]));

        // ... rest of the test remains the same ...
        let info = Info {
            name: "v2_test_torrent".to_string(),
            piece_length: 16384,
            pieces: vec![],
            length: 0,
            files: vec![],
            private: None,
            md5sum: None,
            meta_version: Some(2),
            file_tree: Some(Value::Dict(tree_root)),
        };

        let torrent_input = Torrent {
            info,
            announce: Some("http://tracker.test".to_string()),
            piece_layers: Some(Value::Dict(layers)),
            info_dict_bencode: vec![],
            announce_list: None,
            url_list: None,
            creation_date: None,
            comment: None,
            created_by: None,
            encoding: None,
        };

        let bencoded_data = serde_bencode::to_bytes(&torrent_input).expect("Serialization failed");
        let parsed_torrent = super::from_bytes(&bencoded_data).expect("Parsing failed");

        // ... verify logic ...
        let extracted_roots = parsed_torrent.get_v2_roots();
        let root_map: HashMap<String, Vec<u8>> = extracted_roots.into_iter()
                    .map(|(path, _len, root)| (path, root))
                    .collect();

        assert_eq!(root_map.len(), 2);
        assert_eq!(root_map.get("folder/file_a.txt"), Some(&root_hash_1));
        assert_eq!(root_map.get("file_b.txt"), Some(&root_hash_2));
    }
}

