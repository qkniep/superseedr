// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    pub input: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Add { input: String },
    StopClient,
}

pub fn process_input(input_str: &str, watch_path: &Path) {
    if input_str.starts_with("magnet:") {
        let hash_bytes = Sha1::digest(input_str.as_bytes());
        let file_hash_hex = hex::encode(hash_bytes);

        let final_filename = format!("{}.magnet", file_hash_hex);
        let final_path = watch_path.join(final_filename);
        let temp_filename = format!("{}.magnet.tmp", file_hash_hex);
        let temp_path = watch_path.join(temp_filename);

        tracing::info!(
            "Attempting to write magnet link to temporary path: {:?}",
            temp_path
        );
        match fs::write(&temp_path, input_str.as_bytes()) {
            Ok(_) => {
                tracing::info!(
                    "Atomically renaming magnet file to final path: {:?}",
                    final_path
                );
                if let Err(e) = fs::rename(&temp_path, &final_path) {
                    tracing::error!("Failed to atomically rename magnet file: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to write magnet file to temporary path: {}", e);
            }
        }
    } else {
        let torrent_path = PathBuf::from(input_str);
        match fs::canonicalize(&torrent_path) {
            Ok(absolute_path) => {
                let hash_bytes = Sha1::digest(absolute_path.to_string_lossy().as_bytes());
                let file_hash_hex = hex::encode(hash_bytes);
                let final_filename = format!("{}.path", file_hash_hex);
                let final_dest_path = watch_path.join(final_filename);
                let temp_filename = format!("{}.path.tmp", file_hash_hex);
                let temp_dest_path = watch_path.join(temp_filename);

                let absolute_path_cow = absolute_path.to_string_lossy();
                let content = absolute_path_cow.as_bytes();

                tracing::info!(
                    "Attempting to write torrent path to temporary path: {:?}",
                    temp_dest_path
                );
                match fs::write(&temp_dest_path, content) {
                    Ok(_) => {
                        tracing::info!(
                            "Atomically renaming path file to final path: {:?}",
                            final_dest_path
                        );
                        if let Err(e) = fs::rename(&temp_dest_path, &final_dest_path) {
                            tracing::error!("Failed to atomically rename path file: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to write path file to temporary path: {}", e);
                    }
                }
            }
            Err(e) => {
                // Don't treat as error if launched by macOS without a valid path
                if !input_str.starts_with("magnet:") {
                    // Avoid logging error for magnet links here
                    tracing::warn!(
                        "Input '{}' is not a valid torrent file path: {}",
                        input_str,
                        e
                    );
                }
            }
        }
    }
}
