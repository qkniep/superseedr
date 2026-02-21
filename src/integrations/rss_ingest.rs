// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::{get_watch_path, Settings};
use sha1::{Digest, Sha1};
use std::io;
use std::path::{Path, PathBuf};

pub async fn write_magnet(settings: &Settings, magnet_link: &str) -> io::Result<PathBuf> {
    let watch_dir = rss_watch_dir(settings)?;
    let hash = hex::encode(Sha1::digest(magnet_link.as_bytes()));
    let final_path = watch_dir.join(format!("{}.magnet", hash));
    let temp_path = watch_dir.join(format!("{}.magnet.tmp", hash));

    atomic_write(&temp_path, &final_path, magnet_link.as_bytes()).await?;
    Ok(final_path)
}

pub async fn write_torrent_bytes(
    settings: &Settings,
    source_url: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let watch_dir = rss_watch_dir(settings)?;
    let hash = hex::encode(Sha1::digest(source_url.as_bytes()));
    let final_path = watch_dir.join(format!("{}.torrent", hash));
    let temp_path = watch_dir.join(format!("{}.torrent.tmp", hash));

    atomic_write(&temp_path, &final_path, bytes).await?;
    Ok(final_path)
}

async fn atomic_write(temp_path: &Path, final_path: &Path, payload: &[u8]) -> io::Result<()> {
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(temp_path, payload).await?;
    tokio::fs::rename(temp_path, final_path).await?;
    Ok(())
}

fn rss_watch_dir(settings: &Settings) -> io::Result<PathBuf> {
    if let Some(path) = settings.watch_folder.clone() {
        return Ok(path);
    }

    let (watch_path, _) = get_watch_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "watch path unavailable for RSS auto-ingest",
        )
    })?;
    Ok(watch_path)
}
