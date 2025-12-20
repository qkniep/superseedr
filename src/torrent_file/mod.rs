// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod parser;

use serde::de::{self}; // Import Visitor
use serde::{Deserialize, Deserializer, Serialize};
use serde_bytes::ByteBuf;

use serde_bencode::value::Value;

use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Torrent {
    // This field is special and not directly in the bencode source.
    // We will populate it manually after deserialization.
    #[serde(skip)]
    pub info_dict_bencode: Vec<u8>,

    pub info: Info,
    pub announce: Option<String>,

    #[serde(rename = "announce-list", default)]
    pub announce_list: Option<Vec<Vec<String>>>, // Announce-list is a list of lists of strings

    #[serde(
        rename = "url-list",
        default,
        deserialize_with = "deserialize_url_list"
    )]
    pub url_list: Option<Vec<String>>,

    #[serde(rename = "creation date", default)]
    pub creation_date: Option<i64>, // Creation date is an integer timestamp

    #[serde(default)]
    pub comment: Option<String>,

    #[serde(rename = "created by", default)]
    pub created_by: Option<String>,

    #[serde(default)]
    pub encoding: Option<String>,

    // --- v2 / Hybrid Fields ---

    // Top-level dictionary containing hashes for piece alignment. 
    // Keys are Merkle Roots (32-bytes), Values are the layer hashes.
    // We use `Value` because keys are raw bytes, not UTF-8 strings.
    #[serde(rename = "piece layers", default)]
    pub piece_layers: Option<Value>,
}

impl Torrent {
    /// Extracts all File Merkle Roots found in the v2 `file tree`.
    /// Returns a map of Path -> Root Hash (32 bytes).
    pub fn get_v2_roots(&self) -> Vec<(String, Vec<u8>)> {
        let mut results = Vec::new();
        if let Some(ref tree) = self.info.file_tree {
            // Start traversing from the root of the file tree
            traverse_file_tree(tree, String::new(), &mut results);
        }
        results
    }
}

fn traverse_file_tree(
    node: &Value,
    current_path: String,
    results: &mut Vec<(String, Vec<u8>)>,
) {
    if let Value::Dict(map) = node {
        for (key, value) in map {
            // Keys in `file tree` are filenames (bytes). Convert to string lossy for path display.
            let name = String::from_utf8_lossy(key).to_string();

            if name == "" {
                // An empty key "" indicates this dictionary describes a FILE (Leaf Node).
                // It should contain "pieces root".
                if let Value::Dict(file_metadata) = value {
                    if let Some(Value::Bytes(root)) = file_metadata.get("pieces root".as_bytes()) {
                        results.push((current_path.clone(), root.clone()));
                    }
                }
            } else {
                 // It's a directory or a file node wrapper. Recurse.
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

    // Use serde_bytes to handle this as a raw byte vector
    #[serde(with = "serde_bytes")]
    pub pieces: Vec<u8>,

    #[serde(default)]
    pub private: Option<i64>,

    // `files` is optional (for single-file torrents)
    #[serde(default)]
    pub files: Vec<InfoFile>,

    pub name: String,

    // `length` is optional (for multi-file torrents)
    #[serde(default)]
    pub length: i64,

    #[serde(default)]
    pub md5sum: Option<String>,

    // --- v2 / Hybrid Fields ---

    // Usually '2' for hybrid/v2 torrents
    #[serde(rename = "meta version", default)]
    pub meta_version: Option<i64>,

    // The nested file structure for v2. 
    // We use `Value` to handle the recursive nature and binary keys manually.
    #[serde(rename = "file tree", default)]
    pub file_tree: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InfoFile {
    pub length: i64,
    #[serde(default)]
    pub md5sum: Option<String>,
    // The path is actually a list of strings
    pub path: Vec<String>,
}

fn deserialize_url_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    // 1. Attempt to deserialize the field as a generic Bencode Value
    let v: Value = Deserialize::deserialize(deserializer)?;

    match v {
        // Case A: It's a single string (BEP 19 allows "url-list" to be a string)
        Value::Bytes(bytes) => {
            let s = String::from_utf8(bytes)
                .map_err(|e| de::Error::custom(format!("Invalid UTF-8 in url-list: {}", e)))?;
            Ok(Some(vec![s]))
        }
        // Case B: It's a list of strings (Standard for multi-webseed)
        Value::List(list) => {
            let mut urls = Vec::new();
            for item in list {
                if let Value::Bytes(bytes) = item {
                    let s = String::from_utf8(bytes).map_err(|e| {
                        de::Error::custom(format!("Invalid UTF-8 in url-list: {}", e))
                    })?;
                    urls.push(s);
                }
                // If we encounter non-string items in the list, we skip them or error.
                // Here we strictly expect strings as per spec.
            }
            Ok(Some(urls))
        }
        // Case C: Unexpected type (Int or Dict) - return None or Error
        _ => Ok(None),
    }
}


