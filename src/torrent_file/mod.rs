// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod parser;

use serde::de::{self, Visitor}; // Import Visitor
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt; // Import fmt
use serde_bencode::value::Value;

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
                    let s = String::from_utf8(bytes)
                        .map_err(|e| de::Error::custom(format!("Invalid UTF-8 in url-list: {}", e)))?;
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
