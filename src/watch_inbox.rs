// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::get_watch_path;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn is_cross_device_link_error(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(18) | Some(17))
}

pub fn move_file_with_fallback_impl<F>(
    source: &Path,
    destination: &Path,
    rename_op: F,
) -> io::Result<()>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    match rename_op(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device_link_error(&error) => {
            fs::copy(source, destination)?;
            fs::remove_file(source)?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub fn move_file_with_fallback(source: &Path, destination: &Path) -> io::Result<()> {
    move_file_with_fallback_impl(source, destination, |src, dst| fs::rename(src, dst))
}

pub fn processed_watch_destination(path: &Path) -> Option<PathBuf> {
    let (_, processed_path) = get_watch_path()?;
    let file_name = path.file_name()?;
    Some(processed_path.join(file_name))
}

pub fn archive_watch_file(path: &Path, fallback_extension: &str) -> io::Result<PathBuf> {
    if let Some(destination) = processed_watch_destination(path) {
        if move_file_with_fallback(path, &destination).is_ok() {
            return Ok(destination);
        }
    }

    let mut fallback_path = path.to_path_buf();
    fallback_path.set_extension(fallback_extension);
    fs::rename(path, &fallback_path)?;
    Ok(fallback_path)
}

#[cfg(test)]
mod tests {
    use super::{archive_watch_file, is_cross_device_link_error, move_file_with_fallback_impl};
    use std::fs;

    #[test]
    fn cross_device_link_detection_accepts_windows_and_unix_codes() {
        assert!(is_cross_device_link_error(
            &std::io::Error::from_raw_os_error(18)
        ));
        assert!(is_cross_device_link_error(
            &std::io::Error::from_raw_os_error(17)
        ));
    }

    #[test]
    fn move_file_with_fallback_copies_when_rename_crosses_devices() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("nested").join("destination.txt");
        fs::write(&source, "sample payload").expect("write source file");

        move_file_with_fallback_impl(&source, &destination, |_src, _dst| {
            Err(std::io::Error::from_raw_os_error(17))
        })
        .expect("fallback move should succeed");

        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(&destination).expect("read copied destination"),
            "sample payload"
        );
    }

    #[test]
    fn archive_watch_file_falls_back_to_local_rename_when_processed_dir_is_unavailable() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let source = dir.path().join("sample.control");
        fs::write(&source, "content").expect("write source");

        let archived = archive_watch_file(&source, "control.done").expect("archive watch file");
        assert_eq!(
            archived.extension().and_then(|ext| ext.to_str()),
            Some("done")
        );
    }
}
