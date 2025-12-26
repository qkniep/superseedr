// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod parser;

use serde::de::{self}; 
use serde::{Deserialize, Deserializer, Serialize};
use serde_bencode::value::Value;

// REMOVED: use crate::torrent_file::InfoFile; <-- This caused the circular dependency

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
}

fn traverse_file_tree(
    node: &Value,
    current_path: String,
    results: &mut Vec<(String, u64, Vec<u8>)>,
) {
    if let Value::Dict(map) = node {
        for (key, value) in map {
            let name = String::from_utf8_lossy(key).to_string();

            if name == "" {
                // This is a file metadata node (Leaf)
                if let Value::Dict(file_metadata) = value {
                    // Extract Root
                    if let Some(Value::Bytes(root)) = file_metadata.get("pieces root".as_bytes()) {
                        // Extract Length
                        let len = if let Some(Value::Int(l)) = file_metadata.get("length".as_bytes()) {
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
            if name == "" {
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
