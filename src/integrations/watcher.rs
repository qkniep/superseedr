// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::integrations::control::read_control_request;
use notify::{Config, Error as NotifyError, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{event as tracing_event, Level};

pub fn create_watcher(
    watch_paths: &[PathBuf],
    watch_port_file: bool,
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

    for path in watch_paths {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            tracing_event!(
                Level::ERROR,
                "Failed to watch command path {:?}: {}",
                path,
                e
            );
        } else {
            tracing_event!(Level::INFO, "Watching command path: {:?}", path);
        }
    }

    if watch_port_file {
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
    }

    Ok(watcher)
}

pub fn scan_watch_folder_paths(watch_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for watch_path in watch_paths {
        if let Ok(entries) = fs::read_dir(watch_path) {
            for entry in entries.flatten() {
                paths.push(entry.path());
            }
        } else {
            tracing_event!(
                Level::WARN,
                "Failed to read watch directory: {:?}",
                watch_path
            );
        }
    }

    paths
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

    if path
        .file_name()
        .is_some_and(|name| name == "cluster.revision")
    {
        return Some(AppCommand::ReloadClusterState(path.to_path_buf()));
    }

    let ext = path.extension().and_then(|s| s.to_str())?;
    match ext {
        "torrent" => Some(AppCommand::AddTorrentFromFile(path.to_path_buf())),
        "path" => Some(AppCommand::AddTorrentFromPathFile(path.to_path_buf())),
        "magnet" => Some(AppCommand::AddMagnetFromFile(path.to_path_buf())),
        "control" => match read_control_request(path) {
            Ok(request) => Some(AppCommand::ControlRequest {
                path: path.to_path_buf(),
                request,
            }),
            Err(error) => {
                tracing_event!(
                    Level::WARN,
                    "Failed to parse control request {:?}: {}",
                    path,
                    error
                );
                None
            }
        },
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
    use crate::app::AppCommand;
    use crate::integrations::control::{write_control_request, ControlRequest};
    use notify::EventKind;
    use std::fs::File;
    use std::time::Duration;

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
    fn test_path_to_command_control_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = write_control_request(&ControlRequest::StatusNow, dir.path())
            .expect("write control request");

        let cmd = path_to_command(&path);
        assert!(matches!(
            cmd,
            Some(AppCommand::ControlRequest {
                request: ControlRequest::StatusNow,
                ..
            })
        ));
    }

    #[test]
    fn scan_watch_folder_paths_reads_provided_paths() {
        let dir = std::env::temp_dir().join(format!("watcher_env_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("queued-job.magnet");
        File::create(&file_path).unwrap();

        let paths = scan_watch_folder_paths(std::slice::from_ref(&dir));
        assert!(paths.contains(&file_path));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn create_watcher_emits_live_events_for_provided_watch_paths() {
        let dir = std::env::temp_dir().join(format!("watcher_live_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&dir).unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _watcher = create_watcher(std::slice::from_ref(&dir), false, tx).unwrap();

        let file_path = dir.join("bridge.magnet");
        std::fs::write(
            &file_path,
            "magnet:?xt=urn:btih:1111111111111111111111111111111111111111&dn=LocalWatchProbe",
        )
        .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Some(Ok(event)) if event.paths.iter().any(|path| path == &file_path) => {
                        break event;
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => panic!("watcher error: {error}"),
                    None => panic!("watcher channel closed before receiving file event"),
                }
            }
        })
        .await
        .expect("timed out waiting for watch event");

        assert!(matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_)
        ));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_path_to_command_special_files() {
        // Test regression fix: forwarded_port (no extension)
        with_dummy_file("forwarded_port", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::PortFileChanged(_))));
        });

        with_dummy_file("cluster.revision", |path| {
            let cmd = path_to_command(path);
            assert!(matches!(cmd, Some(AppCommand::ReloadClusterState(_))));
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
