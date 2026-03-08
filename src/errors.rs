// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrackerError {
    #[error("Request failed networking with tracker.")]
    Request(#[from] reqwest::Error),

    #[error("Failed to parse bencoded tracker response")]
    Bencode(#[from] serde_bencode::Error),

    #[error("Tracker returned a failure reason: {0}")]
    Tracker(String),
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    #[error("I/O error ({kind:?}): {message}")]
    Io {
        kind: std::io::ErrorKind,
        message: String,
    },

    #[error("Expected a regular file but found a different filesystem entry")]
    UnexpectedType,

    #[error("Size mismatch: expected {expected_size} bytes, found {observed_size} bytes")]
    SizeMismatch {
        expected_size: u64,
        observed_size: u64,
    },
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

impl StorageError {
    pub fn indicates_data_unavailability(&self) -> bool {
        match self {
            Self::Io { kind, .. } => matches!(
                kind,
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::UnexpectedEof
            ),
            Self::UnexpectedType | Self::SizeMismatch { .. } => true,
        }
    }
}
