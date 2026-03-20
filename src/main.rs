// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

mod app;
mod command;
mod config;
mod config_reconcile;
mod control_service;
mod errors;
mod integrations;
mod integrity_scheduler;
mod networking;
mod persistence;
mod resource_manager;
mod storage;
mod telemetry;
mod theme;
mod token_bucket;
mod torrent_file;
mod torrent_identity;
mod torrent_manager;
mod tracker;
mod tui;
mod tuning;
mod watch_inbox;

use app::App;
use rand::Rng;

use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;

use std::path::PathBuf;
use std::time::Duration;

use crate::config::Settings;
use crate::config::{load_settings, resolve_command_watch_path};
use crate::control_service::{
    apply_offline_control_request, control_event_details, online_control_success_message,
};
use crate::integrations::cli::{
    command_to_control_requests, expand_add_inputs, status_control_request,
    status_file_modified_at, status_should_stream, wait_for_status_json_after,
    write_control_command, write_input_command, write_stop_command, Cli, Commands,
};
use crate::integrations::control::{ControlPriorityTarget, ControlRequest};
use crate::integrations::status::{offline_output_json, status_file_path};
use crate::persistence::event_journal::{
    append_event_journal_entry, event_journal_json, load_event_journal_state,
    save_event_journal_state, ControlOrigin, EventCategory, EventJournalEntry, EventType,
};

use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;

use ratatui::{backend::CrosstermBackend, Terminal};
use std::env;
use std::io::stdout;

use tracing_subscriber::filter::Targets;
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

#[cfg(not(windows))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

use clap::Parser;

const DEFAULT_LOG_FILTER: LevelFilter = LevelFilter::INFO;

// CLI types and process_input moved to integrations::cli

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_data_dir = config::get_app_paths()
        .map(|(_, data_dir)| data_dir)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let log_dir = base_data_dir.join("logs");
    let general_log = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(31)
        .filename_prefix("app")
        .filename_suffix("log")
        .build(&log_dir)
        .expect("Failed to initialize rolling file appender");
    let (non_blocking_general, _guard_general) = tracing_appender::non_blocking(general_log);
    let _subscriber_result = {
        if fs::create_dir_all(&log_dir).is_ok() {
            let quiet_filter = Targets::new()
                .with_default(DEFAULT_LOG_FILTER)
                .with_target("mainline::rpc::socket", LevelFilter::ERROR);

            let general_layer = fmt::layer()
                .with_writer(non_blocking_general)
                .with_ansi(false)
                .with_filter(quiet_filter);

            tracing_subscriber::registry()
                .with(general_layer)
                .try_init()
        } else {
            tracing_subscriber::registry().try_init()
        }
    };

    tracing::info!("STARTING SUPERSEEDR");

    let cli = Cli::parse();
    let loaded_settings = load_settings()?;

    if let Err(e) = config::ensure_watch_directories(&loaded_settings) {
        tracing::error!("Failed to create watch directories: {}", e);
    }

    let has_cli_request = cli.input.is_some() || cli.command.is_some();
    let mut _lock_file_handle = try_acquire_app_lock()?;
    let app_is_running = _lock_file_handle.is_none();

    if has_cli_request {
        if let Err(error) = process_cli_request(&cli, &loaded_settings, app_is_running) {
            eprintln!("[Error] Application failed: {}", error);
            std::process::exit(1);
        }
        tracing::info!("Command processed, exiting temporary instance.");
        return Ok(());
    }

    if _lock_file_handle.is_some() {
        let mut client_configs = loaded_settings;

        #[cfg(all(feature = "dht", feature = "pex"))]
        {
            if client_configs.private_client {
                eprintln!("\n!!!ERROR: POTENTIAL LEAK!!!");
                eprintln!("---------------------------------");
                eprintln!("You are running the normal build of superseedr (with DHT/PEX enabled),");
                eprintln!("but your configuration file indicates you last used a private build.");
                eprintln!("\nThis safety check prevents accidental use of forbidden features on private trackers.");

                // Get the config file path to show the user
                let config_path_str = config::shared_settings_path()
                    .or_else(|| {
                        config::get_app_paths()
                            .map(|(config_dir, _)| config_dir.join("settings.toml"))
                    })
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unable to determine config path.".to_string());

                eprintln!("\nChoose an option:");
                eprintln!("  1. If you want to use the PRIVATE build (for private trackers):");
                eprintln!("     Install and run it:");
                eprintln!("       cargo install superseedr --no-default-features");
                eprintln!("       superseedr");
                eprintln!(
                    "\n  2. If you want to switch back to the NORMAL build (for public trackers):"
                );
                eprintln!("     Manually edit your configuration file:");
                eprintln!("       {}", config_path_str); // Show the path
                eprintln!(
                    "     Change the line `private_client = true` to `private_client = false`"
                );
                eprintln!("     Then, run this normal build again.");

                eprintln!("\nExiting to prevent potential tracker issues.");
                std::process::exit(1);
            }
        }

        #[cfg(not(all(feature = "dht", feature = "pex")))]
        {
            if !client_configs.private_client {
                tracing::info!("Setting private mode flag in configuration.");
                client_configs.private_client = true;
                if let Err(e) = config::save_settings(&client_configs) {
                    tracing::error!(
                        "Failed to save settings after setting private mode flag: {}",
                        e
                    );
                }
            }
        }

        let port_file_path = PathBuf::from("/port-data/forwarded_port");
        tracing::info!("Checking for dynamic port file at {:?}", port_file_path);
        if let Ok(port_str) = fs::read_to_string(&port_file_path) {
            match port_str.trim().parse::<u16>() {
                Ok(dynamic_port) => {
                    if dynamic_port > 0 {
                        tracing::info!(
                            "Successfully read dynamic port {}. Overriding settings.",
                            dynamic_port
                        );
                        client_configs.client_port = dynamic_port;
                    } else {
                        tracing::warn!("Dynamic port file was empty or zero. Using config port.");
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to parse port file content '{}': {}. Using config port.",
                        port_str,
                        e
                    );
                }
            }
        } else {
            tracing::info!(
                "Dynamic file not found. Using port {} from settings.",
                client_configs.client_port
            );
        }

        if client_configs.client_id.is_empty() {
            client_configs.client_id = generate_client_id_string();
            if let Err(e) = config::save_settings(&client_configs) {
                tracing::error!("Failed to save settings after generating client ID: {}", e);
            }
        }

        tracing::info!("Initializing application state...");
        let mut app = App::new(client_configs).await?;
        tracing::info!("Application state initialized. Starting TUI.");

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = cleanup_terminal();
            original_hook(panic_info);
        }));

        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen,)?;
        let _ = execute!(stdout, EnableBracketedPaste);

        #[cfg(not(windows))]
        {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
            );
        }
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        if let Err(e) = app.run(&mut terminal).await {
            eprintln!("[Error] Application failed: {}", e);
        }

        cleanup_terminal()?;
    } else {
        println!("superseedr is already running.");
    }

    Ok(())
}

fn get_lock_path() -> Option<PathBuf> {
    let base_data_dir = config::get_app_paths()
        .map(|(_, data_dir)| data_dir)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    Some(base_data_dir.join("superseedr.lock"))
}

fn try_acquire_app_lock() -> io::Result<Option<File>> {
    let Some(lock_path) = get_lock_path() else {
        return Ok(None);
    };
    let file = File::create(lock_path)?;
    if file.try_lock().is_ok() {
        Ok(Some(file))
    } else {
        Ok(None)
    }
}

fn process_cli_request(cli: &Cli, settings: &Settings, app_is_running: bool) -> io::Result<()> {
    if let Some(direct_input) = &cli.input {
        let watch_path = resolve_command_watch_path(settings).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not resolve the command watch path",
            )
        })?;
        tracing::info!("Processing direct input: {}", direct_input);
        let command_path = write_input_command(direct_input, &watch_path)?;
        println!("Queued add command at {}", command_path.display());
        return Ok(());
    }

    let Some(command) = &cli.command else {
        return Ok(());
    };

    match command {
        Commands::Add { inputs } => {
            let watch_path = resolve_command_watch_path(settings).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "Could not resolve the command watch path",
                )
            })?;
            for input in expand_add_inputs(inputs) {
                tracing::info!("Processing Add subcommand input: {}", input);
                let command_path = write_input_command(&input, &watch_path)?;
                println!("Queued add command at {}", command_path.display());
            }
            Ok(())
        }
        Commands::Journal => {
            println!("{}", event_journal_json()?);
            Ok(())
        }
        Commands::Status { .. } => {
            let request = status_control_request(command)
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
            if app_is_running {
                process_online_status_request(settings, &request, status_should_stream(command))
            } else {
                process_offline_control_request(settings, &request)
            }
        }
        Commands::StopClient => {
            if !app_is_running {
                println!("superseedr is not running.");
                return Ok(());
            }
            let watch_path = resolve_command_watch_path(settings).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "Could not resolve the command watch path",
                )
            })?;
            tracing::info!("Processing StopClient command.");
            let _ = write_stop_command(&watch_path)?;
            println!("Queued stop request.");
            Ok(())
        }
        _ => {
            let requests = command_to_control_requests(command)
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Unsupported command")
                })?;

            for request in requests {
                if app_is_running {
                    process_online_control_request(settings, &request)?;
                } else {
                    process_offline_control_request(settings, &request)?;
                }
            }
            Ok(())
        }
    }
}

fn process_online_status_request(
    settings: &Settings,
    request: &ControlRequest,
    stream: bool,
) -> io::Result<()> {
    let watch_path = resolve_command_watch_path(settings).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not resolve the command watch path",
        )
    })?;

    match request {
        ControlRequest::StatusNow => {
            let previous_modified_at = status_file_modified_at()?;
            let _ = write_control_command(request, &watch_path)?;
            let json = wait_for_status_json_after(previous_modified_at, Duration::from_secs(15))?;
            println!("{}", json);
            Ok(())
        }
        ControlRequest::StatusFollowStart { interval_secs } if stream => {
            let mut last_modified_at = status_file_modified_at()?;
            let _ = write_control_command(request, &watch_path)?;
            loop {
                let json = wait_for_status_json_after(
                    last_modified_at,
                    Duration::from_secs(interval_secs.saturating_mul(3).max(15)),
                )?;
                println!("{}", json);
                io::stdout().flush()?;
                last_modified_at = status_file_modified_at()?;
            }
        }
        ControlRequest::StatusFollowStart { interval_secs } => {
            let _ = write_control_command(request, &watch_path)?;
            let status_path = status_file_path()?;
            println!(
                "Set status output interval to {} seconds.\nStatus file: {}",
                interval_secs,
                status_path.display()
            );
            Ok(())
        }
        ControlRequest::StatusFollowStop => {
            let _ = write_control_command(request, &watch_path)?;
            println!("Queued status streaming stop request.");
            Ok(())
        }
        _ => unreachable!("status request handler received non-status control request"),
    }
}

fn process_online_control_request(settings: &Settings, request: &ControlRequest) -> io::Result<()> {
    let watch_path = resolve_command_watch_path(settings).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not resolve the command watch path",
        )
    })?;

    match request {
        ControlRequest::StatusNow => {
            let previous_modified_at = status_file_modified_at()?;
            let _ = write_control_command(request, &watch_path)?;
            let json = wait_for_status_json_after(previous_modified_at, Duration::from_secs(15))?;
            println!("{}", json);
            Ok(())
        }
        ControlRequest::StatusFollowStart { interval_secs } => {
            let mut last_modified_at = status_file_modified_at()?;
            let _ = write_control_command(request, &watch_path)?;
            loop {
                let json = wait_for_status_json_after(
                    last_modified_at,
                    Duration::from_secs(interval_secs.saturating_mul(3).max(15)),
                )?;
                println!("{}", json);
                io::stdout().flush()?;
                last_modified_at = status_file_modified_at()?;
            }
        }
        ControlRequest::StatusFollowStop => {
            let _ = write_control_command(request, &watch_path)?;
            println!("Queued status streaming stop request.");
            Ok(())
        }
        _ => {
            let _ = write_control_command(request, &watch_path)?;
            println!("{}", online_control_success_message(request));
            Ok(())
        }
    }
}

fn process_offline_control_request(
    settings: &Settings,
    request: &ControlRequest,
) -> io::Result<()> {
    match request {
        ControlRequest::StatusNow => {
            println!("{}", offline_output_json(settings)?);
            return Ok(());
        }
        ControlRequest::StatusFollowStart { .. } | ControlRequest::StatusFollowStop => {
            return Err(io::Error::other(
                "Streaming status commands require a running superseedr instance",
            ));
        }
        _ => {}
    }

    let mut next_settings = settings.clone();
    let mut result = apply_offline_control_request(&mut next_settings, request);
    if result.is_ok() {
        if let Err(error) = config::save_settings(&next_settings) {
            result = Err(format!("Failed to save updated settings: {}", error));
        }
    }
    record_offline_control_journal_entry(request, &result);
    let message = result.map_err(io::Error::other)?;
    println!("{}", message);
    Ok(())
}

fn record_offline_control_journal_entry(request: &ControlRequest, result: &Result<String, String>) {
    let mut journal = load_event_journal_state();
    let event_type = if result.is_ok() {
        EventType::ControlApplied
    } else {
        EventType::ControlFailed
    };
    let message = match result {
        Ok(message) | Err(message) => Some(message.clone()),
    };
    append_event_journal_entry(
        &mut journal,
        EventJournalEntry {
            host_id: config::shared_host_id(),
            ts_iso: chrono::Utc::now().to_rfc3339(),
            category: EventCategory::Control,
            event_type,
            message,
            details: control_event_details(request, ControlOrigin::CliOffline),
            ..Default::default()
        },
    );
    if let Err(error) = save_event_journal_state(&journal) {
        tracing::error!("Failed to save offline control journal entry: {}", error);
    }
}

fn cleanup_terminal() -> Result<(), Box<dyn std::error::Error>> {
    let _ = disable_raw_mode();
    // Common cleanup for all platforms
    let _ = execute!(stdout(), LeaveAlternateScreen,);
    let _ = execute!(stdout(), DisableBracketedPaste);

    #[cfg(not(windows))]
    {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }

    Ok(())
}

fn generate_client_id_string() -> String {
    const CLIENT_PREFIX: &str = "-SS1000-";
    const RANDOM_LEN: usize = 12;

    let mut rng = rand::rng();
    let random_chars: String = (0..RANDOM_LEN)
        .map(|_| {
            const CHARSET: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    format!("{}{}", CLIENT_PREFIX, random_chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_settings() -> Settings {
        Settings {
            torrents: vec![config::TorrentSettings {
                torrent_or_magnet: "magnet:?xt=urn:btih:1111111111111111111111111111111111111111"
                    .to_string(),
                name: "Sample Alpha".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn write_sample_torrent_file() -> (tempfile::TempDir, String) {
        let dir = tempdir().expect("create tempdir");
        let torrent = crate::torrent_file::Torrent {
            info: crate::torrent_file::Info {
                name: "sample-pack".to_string(),
                piece_length: 16_384,
                pieces: vec![0; 20],
                files: vec![
                    crate::torrent_file::InfoFile {
                        length: 10,
                        path: vec!["folder".to_string(), "alpha.bin".to_string()],
                        md5sum: None,
                        attr: None,
                    },
                    crate::torrent_file::InfoFile {
                        length: 20,
                        path: vec!["folder".to_string(), "beta.bin".to_string()],
                        md5sum: None,
                        attr: None,
                    },
                ],
                ..Default::default()
            },
            announce: Some("http://tracker.test".to_string()),
            ..Default::default()
        };
        let bytes = serde_bencode::to_bytes(&torrent).expect("serialize torrent");
        let path = dir
            .path()
            .join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.torrent");
        fs::write(&path, bytes).expect("write torrent fixture");
        (dir, path.to_string_lossy().to_string())
    }

    #[test]
    fn offline_pause_updates_torrent_control_state() {
        let mut settings = sample_settings();
        let request = ControlRequest::Pause {
            info_hash_hex: "1111111111111111111111111111111111111111".to_string(),
        };

        let result = apply_offline_control_request(&mut settings, &request);

        assert!(result.is_ok());
        assert_eq!(
            settings.torrents[0].torrent_control_state,
            app::TorrentControlState::Paused
        );
    }

    #[test]
    fn offline_delete_removes_matching_torrent() {
        let mut settings = sample_settings();
        let request = ControlRequest::Delete {
            info_hash_hex: "1111111111111111111111111111111111111111".to_string(),
        };

        let result = apply_offline_control_request(&mut settings, &request);

        assert!(result.is_ok());
        assert!(settings.torrents.is_empty());
    }

    #[test]
    fn offline_resume_updates_torrent_control_state() {
        let mut settings = sample_settings();
        settings.torrents[0].torrent_control_state = app::TorrentControlState::Paused;
        let request = ControlRequest::Resume {
            info_hash_hex: "1111111111111111111111111111111111111111".to_string(),
        };

        let result = apply_offline_control_request(&mut settings, &request);

        assert!(result.is_ok());
        assert_eq!(
            settings.torrents[0].torrent_control_state,
            app::TorrentControlState::Running
        );
    }

    #[test]
    fn offline_priority_updates_file_priority_by_index() {
        let (_dir, torrent_path) = write_sample_torrent_file();
        let mut settings = Settings {
            torrents: vec![config::TorrentSettings {
                torrent_or_magnet: torrent_path,
                name: "Sample Pack".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let request = ControlRequest::SetFilePriority {
            info_hash_hex: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            target: ControlPriorityTarget::FileIndex(1),
            priority: app::FilePriority::High,
        };

        let result = apply_offline_control_request(&mut settings, &request);

        assert!(result.is_ok());
        assert_eq!(
            settings.torrents[0].file_priorities.get(&1),
            Some(&app::FilePriority::High)
        );
    }

    #[test]
    fn offline_priority_updates_file_priority_by_relative_path() {
        let (_dir, torrent_path) = write_sample_torrent_file();
        let mut settings = Settings {
            torrents: vec![config::TorrentSettings {
                torrent_or_magnet: torrent_path,
                name: "Sample Pack".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let request = ControlRequest::SetFilePriority {
            info_hash_hex: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            target: ControlPriorityTarget::FilePath("folder/beta.bin".to_string()),
            priority: app::FilePriority::Skip,
        };

        let result = apply_offline_control_request(&mut settings, &request);

        assert!(result.is_ok());
        assert_eq!(
            settings.torrents[0].file_priorities.get(&1),
            Some(&app::FilePriority::Skip)
        );
    }
}
