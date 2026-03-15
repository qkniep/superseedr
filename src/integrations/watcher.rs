// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::config::{
    configured_watch_paths, is_shared_config_path, shared_config_watch_paths, Settings,
};
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

    for path in configured_watch_paths(settings) {
        if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
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

    for shared_path in shared_config_watch_paths() {
        if let Err(e) = watcher.watch(&shared_path, RecursiveMode::NonRecursive) {
            tracing_event!(
                Level::WARN,
                "Failed to watch shared config path {:?}: {}",
                shared_path,
                e
            );
        } else {
            tracing_event!(
                Level::INFO,
                "Watching shared config path: {:?}",
                shared_path
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

pub fn scan_watch_folder_paths(settings: &Settings) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for watch_path in configured_watch_paths(settings) {
        if let Ok(entries) = fs::read_dir(&watch_path) {
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

pub fn scan_watch_folders(settings: &Settings) -> Vec<AppCommand> {
    scan_watch_folder_paths(settings)
        .into_iter()
        .filter_map(|path| path_to_command(&path))
        .collect()
}

pub fn path_to_command(path: &Path) -> Option<AppCommand> {
    if is_shared_config_path(path) {
        return Some(AppCommand::ReloadSharedConfig);
    }

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
    use crate::app::AppCommand;
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

    fn watch_env_guard() -> &'static std::sync::Mutex<()> {
        static GUARD: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        GUARD.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn scan_watch_folders_reads_env_watch_paths() {
        let _guard = watch_env_guard().lock().unwrap();
        let original = std::env::var_os("SUPERSEEDR_WATCH_PATH_1");

        let dir = std::env::temp_dir().join(format!("watcher_env_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("queued-job.magnet");
        File::create(&file_path).unwrap();
        std::env::set_var("SUPERSEEDR_WATCH_PATH_1", &dir);

        let commands = scan_watch_folders(&crate::config::Settings::default());
        assert!(commands
            .iter()
            .any(|cmd| matches!(cmd, AppCommand::AddMagnetFromFile(path) if path == &file_path)));

        if let Some(value) = original {
            std::env::set_var("SUPERSEEDR_WATCH_PATH_1", value);
        } else {
            std::env::remove_var("SUPERSEEDR_WATCH_PATH_1");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn create_watcher_emits_live_events_for_env_watch_paths() {
        let _guard = watch_env_guard().lock().unwrap();
        let original = std::env::var_os("SUPERSEEDR_WATCH_PATH_1");

        let dir = std::env::temp_dir().join(format!("watcher_live_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("SUPERSEEDR_WATCH_PATH_1", &dir);

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _watcher = create_watcher(&crate::config::Settings::default(), tx).unwrap();

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

        assert!(matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)));

        if let Some(value) = original {
            std::env::set_var("SUPERSEEDR_WATCH_PATH_1", value);
        } else {
            std::env::remove_var("SUPERSEEDR_WATCH_PATH_1");
        }
        let _ = fs::remove_dir_all(dir);
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



