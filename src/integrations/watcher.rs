// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::config::{get_watch_path, Settings};
use notify::{Config, Error as NotifyError, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{event as tracing_event, Level};

pub fn create_watcher(
    settings: &Settings,
    tx: mpsc::Sender<Result<Event, NotifyError>>,
) -> Result<RecommendedWatcher, Box<dyn std::error::Error>> {
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, NotifyError>| {
            if let Err(e) = tx.blocking_send(res) {
                tracing_event!(Level::ERROR, "Failed to send file event: {}", e);
            }
        },
        Config::default(),
    )?;

    if let Some(path) = &settings.watch_folder {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            tracing_event!(Level::ERROR, "Failed to watch user path {:?}: {}", path, e);
        } else {
            tracing_event!(Level::INFO, "Watching user path: {:?}", path);
        }
    }

    if let Some((watch_path, _)) = get_watch_path() {
        if let Err(e) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
            tracing_event!(
                Level::ERROR,
                "Failed to watch system path {:?}: {}",
                watch_path,
                e
            );
        }
    }

    let port_file_path = PathBuf::from("/port-data/forwarded_port");
    if let Some(port_dir) = port_file_path.parent() {
        if let Err(e) = watcher.watch(port_dir, RecursiveMode::NonRecursive) {
            tracing_event!(
                Level::WARN,
                "Failed to watch port file directory {:?}: {}",
                port_dir,
                e
            );
        } else {
            tracing_event!(
                Level::INFO,
                "Watching for port file changes in {:?}",
                port_dir
            );
        }
    }

    Ok(watcher)
}

pub fn scan_watch_folders(settings: &Settings) -> Vec<AppCommand> {
    let mut commands = Vec::new();

    if let Some(user_watch_path) = &settings.watch_folder {
        if let Ok(entries) = fs::read_dir(user_watch_path) {
            for entry in entries.flatten() {
                if let Some(cmd) = path_to_command(&entry.path()) {
                    commands.push(cmd);
                }
            }
        } else {
            tracing_event!(
                Level::WARN,
                "Failed to read user watch directory: {:?}",
                user_watch_path
            );
        }
    }

    if let Some((watch_path, _)) = get_watch_path() {
        if let Ok(entries) = fs::read_dir(watch_path) {
            for entry in entries.flatten() {
                if let Some(cmd) = path_to_command(&entry.path()) {
                    commands.push(cmd);
                }
            }
        }
    }

    commands
}

pub fn path_to_command(path: &Path) -> Option<AppCommand> {
    if !path.is_file() {
        return None;
    }

    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|s| s.starts_with('.'))
    {
        return None;
    }

    if path.to_string_lossy().ends_with(".tmp") {
        return None;
    }

    if path
        .file_name()
        .is_some_and(|name| name == "forwarded_port")
    {
        return Some(AppCommand::PortFileChanged(path.to_path_buf()));
    }

    let ext = path.extension().and_then(|s| s.to_str())?;
    match ext {
        "torrent" => Some(AppCommand::AddTorrentFromFile(path.to_path_buf())),
        "path" => Some(AppCommand::AddTorrentFromPathFile(path.to_path_buf())),
        "magnet" => Some(AppCommand::AddMagnetFromFile(path.to_path_buf())),
        "cmd" if path.file_name().is_some_and(|name| name == "shutdown.cmd") => {
            Some(AppCommand::ClientShutdown(path.to_path_buf()))
        }
        _ if path
            .file_name()
            .is_some_and(|name| name == "forwarded_port") =>
        {
            Some(AppCommand::PortFileChanged(path.to_path_buf()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use crate::app::AppCommand;

    // Helper to create a dummy file for testing (since path_to_command checks is_file())
    fn with_dummy_file<F>(name: &str, test_fn: F)
    where
        F: FnOnce(&Path),
    {
        let dir = std::env::temp_dir().join(format!("watcher_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join(name);
        File::create(&file_path).unwrap();

        test_fn(&file_path);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_path_to_command_extensions() {
        with_dummy_file("ubuntu.torrent", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::AddTorrentFromFile(_))));
        });

        with_dummy_file("meta.magnet", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::AddMagnetFromFile(_))));
        });

        with_dummy_file("job.path", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::AddTorrentFromPathFile(_))));
        });
    }

    #[test]
    fn test_path_to_command_special_files() {
        // Test regression fix: forwarded_port (no extension)
        with_dummy_file("forwarded_port", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::PortFileChanged(_))));
        });

        // Test shutdown command
        with_dummy_file("shutdown.cmd", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::ClientShutdown(_))));
        });
    }

    #[test]
    fn test_path_to_command_ignored() {
        // .tmp files should be ignored
        with_dummy_file("file.torrent.tmp", |path| {
            assert!(path_to_command(path).is_none());
        });

        // Random extensions should be ignored
        with_dummy_file("image.png", |path| {
            assert!(path_to_command(path).is_none());
        });

        // Directories should be ignored
        let dir = std::env::temp_dir().join("test_dir_ignore");
        fs::create_dir_all(&dir).unwrap();
        assert!(path_to_command(&dir).is_none());
        let _ = fs::remove_dir(dir);
    }
}
