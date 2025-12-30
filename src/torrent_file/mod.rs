// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod parser;

use serde::de::{self};
use serde::{Deserialize, Deserializer, Serialize};
use serde_bencode::value::Value;

use std::collections::HashMap;


pub struct V2Mapping {
    /// Maps global piece indices to a list of file roots/offsets
    pub piece_to_roots: HashMap<u32, Vec<(u64, u64, Vec<u8>)>>,
    /// Total count of aligned pieces for a Pure V2 torrent
    pub piece_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Torrent {
    // This field is special and not directly in the bencode source.
    // We will populate it manually after deserialization.
    #[serde(skip)]
    pub info_dict_bencode: Vec<u8>,

    pub info: Info,
    pub announce: Option<String>,

    #[serde(rename = "announce-list", default)]
    pub announce_list: Option<Vec<Vec<String>>>,

    #[serde(
        rename = "url-list",
        default,
        deserialize_with = "deserialize_url_list"
    )]
    pub url_list: Option<Vec<String>>,

    #[serde(rename = "creation date", default)]
    pub creation_date: Option<i64>,

    #[serde(default)]
    pub comment: Option<String>,

    #[serde(rename = "created by", default)]
    pub created_by: Option<String>,

    #[serde(default)]
    pub encoding: Option<String>,

    // --- v2 / Hybrid Fields ---
    #[serde(rename = "piece layers", default)]
    pub piece_layers: Option<Value>,
}



impl Torrent {
    pub fn get_v2_roots(&self) -> Vec<(String, u64, Vec<u8>)> {
        let mut results = Vec::new();
        if let Some(ref tree) = self.info.file_tree {
            traverse_file_tree(tree, String::new(), &mut results);
        }
        results
    }

    pub fn get_layer_hashes(&self, root_hash: &[u8]) -> Option<Vec<u8>> {
        if let Some(Value::Dict(layers)) = &self.piece_layers {
            if let Some(Value::Bytes(layer_data)) = layers.get(root_hash) {
                return Some(layer_data.clone());
            }
        }
        None
    }

    pub fn calculate_v2_mapping(&self) -> V2Mapping {
        let mut piece_to_roots: HashMap<u32, Vec<(u64, u64, Vec<u8>)>> = HashMap::new();
        let piece_len = self.info.piece_length as u64;
        let mut current_piece_index = 0;

        // V2 pieces are aligned to file boundaries
        if self.info.meta_version == Some(2) && piece_len > 0 {
            let mut v2_roots = self.get_v2_roots(); 

            // Critical: Sort files by path to ensure deterministic mapping
            v2_roots.sort_by(|(path_a, _, _), (path_b, _, _)| path_a.cmp(path_b));

            for (_path, length, root_hash) in v2_roots {
                if length > 0 {
                    let file_pieces = length.div_ceil(piece_len);
                    let file_start_offset = current_piece_index * piece_len;

                    let start_piece = current_piece_index as u32;
                    let end_piece = (current_piece_index + file_pieces) as u32;

                    for p in start_piece..end_piece {
                        piece_to_roots.entry(p).or_default().push((
                            file_start_offset,
                            length,
                            root_hash.clone(),
                        ));
                    }
                    current_piece_index += file_pieces;
                }
            }
        }

        V2Mapping {
            piece_to_roots,
            piece_count: current_piece_index as usize,
        }
    }

    /// Returns the total piece count, prioritizing V1 pieces string but falling back to V2
    pub fn total_piece_count(&self, v2_count: usize) -> usize {
        if !self.info.pieces.is_empty() {
            // V1 / Hybrid: Use the explicit pieces string
            self.info.pieces.len() / 20
        } else if v2_count > 0 {
            // Pure V2: Use the calculated aligned count
            v2_count
        } else {
            // Fallback: Standard length calculation
            let total_len = self.info.total_length() as u64;
            let pl = self.info.piece_length as u64;
            if pl > 0 { total_len.div_ceil(pl) as usize } else { 0 }
        }
    }
}

fn traverse_file_tree(
    node: &Value,
    current_path: String,
    results: &mut Vec<(String, u64, Vec<u8>)>,
) {
    if let Value::Dict(map) = node {
        for (key, value) in map {
            let name = String::from_utf8_lossy(key).to_string();

            if name.is_empty() {
                // This is a file metadata node (Leaf)
                if let Value::Dict(file_metadata) = value {
                    // Extract Root
                    if let Some(Value::Bytes(root)) = file_metadata.get("pieces root".as_bytes()) {
                        // Extract Length
                        let len =
                            if let Some(Value::Int(l)) = file_metadata.get("length".as_bytes()) {
                                *l as u64
                            } else {
                                0
                            };
                        results.push((current_path.clone(), len, root.clone()));
                    }
                }
            } else {
                // Directory node
                let new_path = if current_path.is_empty() {
                    name
                } else {
                    format!("{}/{}", current_path, name)
                };
                traverse_file_tree(value, new_path, results);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Info {
    #[serde(rename = "piece length")]
    pub piece_length: i64,

    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub pieces: Vec<u8>,

    #[serde(default)]
    pub private: Option<i64>,

    #[serde(default)]
    pub files: Vec<InfoFile>,

    pub name: String,

    #[serde(default)]
    pub length: i64,

    #[serde(default)]
    pub md5sum: Option<String>,

    // --- v2 / Hybrid Fields ---
    #[serde(rename = "meta version", default)]
    pub meta_version: Option<i64>,

    #[serde(rename = "file tree", default)]
    pub file_tree: Option<Value>,
}

impl Info {
    pub fn total_length(&self) -> i64 {
        // Case 1: v1 Single File
        if self.length > 0 {
            return self.length;
        }

        // Case 2: v1 Multi-File
        if !self.files.is_empty() {
            return self.files.iter().map(|f| f.length).sum();
        }

        // Case 3: v2 File Tree
        if let Some(ref tree) = self.file_tree {
            return calculate_tree_size(tree);
        }

        0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InfoFile {
    pub length: i64,

    #[serde(default)]
    pub md5sum: Option<String>,

    pub path: Vec<String>,

    #[serde(default)]
    pub attr: Option<String>,
}

fn deserialize_url_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Value = Deserialize::deserialize(deserializer)?;

    match v {
        Value::Bytes(bytes) => {
            let s = String::from_utf8(bytes)
                .map_err(|e| de::Error::custom(format!("Invalid UTF-8 in url-list: {}", e)))?;
            Ok(Some(vec![s]))
        }
        Value::List(list) => {
            let mut urls = Vec::new();
            for item in list {
                if let Value::Bytes(bytes) = item {
                    let s = String::from_utf8(bytes).map_err(|e| {
                        de::Error::custom(format!("Invalid UTF-8 in url-list: {}", e))
                    })?;
                    urls.push(s);
                }
            }
            Ok(Some(urls))
        }
        _ => Ok(None),
    }
}

fn calculate_tree_size(node: &Value) -> i64 {
    let mut size = 0;
    if let Value::Dict(map) = node {
        for (key, value) in map {
            let name = String::from_utf8_lossy(key);
            if name.is_empty() {
                // This is a file metadata node
                if let Value::Dict(meta) = value {
                    if let Some(Value::Int(len)) = meta.get("length".as_bytes()) {
                        size += len;
                    }
                }
            } else {
                // This is a subdirectory or file entry, recurse
                size += calculate_tree_size(value);
            }
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Helper to create a basic Info object
    fn create_test_info(meta_version: Option<i64>) -> Info {
        Info {
            piece_length: 16384,
            pieces: Vec::new(),
            private: None,
            files: Vec::new(),
            name: "test_torrent".to_string(),
            length: 0,
            md5sum: None,
            meta_version,
            file_tree: None,
        }
    }

    // Helper to build a v2 file tree node
    fn build_v2_file_node(length: i64, root: Vec<u8>) -> Value {
        let mut meta = HashMap::new();
        meta.insert("length".as_bytes().to_vec(), Value::Int(length));
        meta.insert("pieces root".as_bytes().to_vec(), Value::Bytes(root));
        
        let mut leaf = HashMap::new();
        leaf.insert(vec![], Value::Dict(meta));
        Value::Dict(leaf)
    }

    #[test]
    fn test_v2_piece_count_calculation() {
        // Setup: 2 files, each 1000 bytes. Piece length 16384.
        // In V2, files are aligned to piece boundaries, so this should be 2 pieces.
        let mut torrent = Torrent {
            info_dict_bencode: vec![],
            info: create_test_info(Some(2)),
            announce: None,
            announce_list: None,
            url_list: None,
            creation_date: None,
            comment: None,
            created_by: None,
            encoding: None,
            piece_layers: None,
        };

        let mut tree = HashMap::new();
        tree.insert("a.txt".as_bytes().to_vec(), build_v2_file_node(1000, vec![0xAA; 32]));
        tree.insert("b.txt".as_bytes().to_vec(), build_v2_file_node(1000, vec![0xBB; 32]));
        torrent.info.file_tree = Some(Value::Dict(tree));

        let mapping = torrent.calculate_v2_mapping();
        
        // Assertions
        assert_eq!(mapping.piece_count, 2);
        assert_eq!(torrent.total_piece_count(mapping.piece_count), 2);
        
        // Verify Piece 0 belongs to File A and Piece 1 to File B
        let roots_0 = mapping.piece_to_roots.get(&0).unwrap();
        let roots_1 = mapping.piece_to_roots.get(&1).unwrap();
        assert_eq!(roots_0[0].2, vec![0xAA; 32]);
        assert_eq!(roots_1[0].2, vec![0xBB; 32]);
    }

    #[test]
    fn test_hybrid_piece_count_prioritizes_v1_string() {
        // Setup: Hybrid torrent with a V1 pieces string (length for 10 pieces)
        // and a V2 mapping that suggests 5 pieces.
        let mut torrent = Torrent {
            info_dict_bencode: vec![],
            info: create_test_info(Some(2)),
            announce: None,
            announce_list: None,
            url_list: None,
            creation_date: None,
            comment: None,
            created_by: None,
            encoding: None,
            piece_layers: None,
        };
        
        // V1 pieces string: 20 bytes * 10 pieces
        torrent.info.pieces = vec![0u8; 200];
        
        // V2 mapping says 5 pieces
        let v2_count = 5;
        
        // The result MUST be 10 because the swarm uses V1 piece indices in Hybrid mode.
        assert_eq!(torrent.total_piece_count(v2_count), 10);
    }

    #[test]
    fn test_deterministic_v2_sorting() {
        // Setup: Add files in a specific order. The sorting should ensure they are
        // mapped to pieces alphabetically regardless of insertion order.
        let mut torrent = Torrent {
            info_dict_bencode: vec![],
            info: create_test_info(Some(2)),
            announce: None,
            announce_list: None,
            url_list: None,
            creation_date: None,
            comment: None,
            created_by: None,
            encoding: None,
            piece_layers: None,
        };

        let mut tree = HashMap::new();
        // Insert 'z.txt' then 'a.txt'. 
        // Use 0x5A (ASCII 'Z') instead of the invalid 0xZZ.
        tree.insert("z.txt".as_bytes().to_vec(), build_v2_file_node(1000, vec![0x5A; 32]));
        tree.insert("a.txt".as_bytes().to_vec(), build_v2_file_node(1000, vec![0xAA; 32]));
        torrent.info.file_tree = Some(Value::Dict(tree));

        let mapping = torrent.calculate_v2_mapping();
        
        // Piece 0 must be 'a.txt' (0xAA) due to alphabetical sorting
        let roots_0 = mapping.piece_to_roots.get(&0).expect("Piece 0 mapping missing");
        assert_eq!(roots_0[0].2, vec![0xAA; 32]);
        
        // Piece 1 must be 'z.txt' (0x5A)
        let roots_1 = mapping.piece_to_roots.get(&1).expect("Piece 1 mapping missing");
        assert_eq!(roots_1[0].2, vec![0x5A; 32]);
    }

    #[test]
    fn test_v2_mapping_with_empty_files() {
        // Setup: V2 file tree with one empty file and one real file.
        // Empty files do not have piece roots and should not increment piece index.
        let mut torrent = Torrent {
            info_dict_bencode: vec![],
            info: create_test_info(Some(2)),
            announce: None,
            announce_list: None,
            url_list: None,
            creation_date: None,
            comment: None,
            created_by: None,
            encoding: None,
            piece_layers: None,
        };

        let mut tree = HashMap::new();
        // "empty.txt" length 0
        tree.insert("empty.txt".as_bytes().to_vec(), build_v2_file_node(0, vec![0x00; 32]));
        // "real.txt" length 1000
        tree.insert("real.txt".as_bytes().to_vec(), build_v2_file_node(1000, vec![0xAA; 32]));
        torrent.info.file_tree = Some(Value::Dict(tree));

        let mapping = torrent.calculate_v2_mapping();
        
        // Only 1 piece should exist (for real.txt)
        assert_eq!(mapping.piece_count, 1);
        assert_eq!(mapping.piece_to_roots.get(&0).unwrap()[0].2, vec![0xAA; 32]);
    }
}
