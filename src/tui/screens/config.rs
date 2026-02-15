// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use crate::app::{AppCommand, ConfigItem, FileBrowserMode};
use crate::config::Settings;
use crate::token_bucket::TokenBucket;
use directories::UserDirs;
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use tokio::sync::mpsc;

pub enum ConfigOutcome {
    Stay,
    ToNormal,
}

pub fn handle_event(
    event: CrosstermEvent,
    settings_edit: &mut Box<Settings>,
    selected_index: &mut usize,
    items: &mut [ConfigItem],
    editing: &mut Option<(ConfigItem, String)>,
    app_command_tx: &mpsc::Sender<AppCommand>,
    global_dl_bucket: &Arc<TokenBucket>,
    global_ul_bucket: &Arc<TokenBucket>,
) -> ConfigOutcome {
    if let Some((item, buffer)) = editing {
        if let CrosstermEvent::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char(c) => {
                        if c.is_ascii_digit() {
                            buffer.push(c);
                        }
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Esc => *editing = None,
                    KeyCode::Enter => {
                        match item {
                            ConfigItem::ClientPort => {
                                if let Ok(new_port) = buffer.parse::<u16>() {
                                    if new_port > 0 {
                                        settings_edit.client_port = new_port;
                                    }
                                }
                            }
                            ConfigItem::GlobalDownloadLimit => {
                                if let Ok(new_rate) = buffer.parse::<u64>() {
                                    settings_edit.global_download_limit_bps = new_rate;
                                    let bucket = global_dl_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                            }
                            ConfigItem::GlobalUploadLimit => {
                                if let Ok(new_rate) = buffer.parse::<u64>() {
                                    settings_edit.global_upload_limit_bps = new_rate;
                                    let bucket = global_ul_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                            }
                            _ => {}
                        }
                        *editing = None;
                    }
                    _ => {}
                }
            }
        }
        return ConfigOutcome::Stay;
    }

    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Esc | KeyCode::Char('Q') => {
                    let _ = app_command_tx.try_send(AppCommand::UpdateConfig(*settings_edit.clone()));
                    return ConfigOutcome::ToNormal;
                }
                KeyCode::Enter => {
                    let selected_item = items[*selected_index];
                    match selected_item {
                        ConfigItem::GlobalDownloadLimit
                        | ConfigItem::GlobalUploadLimit
                        | ConfigItem::ClientPort => {
                            *editing = Some((selected_item, String::new()));
                        }
                        ConfigItem::DefaultDownloadFolder | ConfigItem::WatchFolder => {
                            let initial_path = if selected_item == ConfigItem::WatchFolder {
                                settings_edit.watch_folder.clone()
                            } else {
                                settings_edit.default_download_folder.clone()
                            }
                            .unwrap_or_else(|| {
                                UserDirs::new()
                                    .and_then(|ud| ud.download_dir().map(|p| p.to_path_buf()))
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                            });

                            let _ = app_command_tx.try_send(AppCommand::FetchFileTree {
                                path: initial_path,
                                browser_mode: FileBrowserMode::ConfigPathSelection {
                                    target_item: selected_item,
                                    current_settings: settings_edit.clone(),
                                    selected_index: *selected_index,
                                    items: items.to_vec(),
                                },
                                highlight_path: None,
                            });
                        }
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected_index = selected_index.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected_index < items.len() - 1 {
                        *selected_index += 1;
                    }
                }
                KeyCode::Char('r') => {
                    let default_settings = Settings::default();
                    let selected_item = items[*selected_index];
                    match selected_item {
                        ConfigItem::ClientPort => {
                            settings_edit.client_port = default_settings.client_port;
                        }
                        ConfigItem::DefaultDownloadFolder => {
                            settings_edit.default_download_folder =
                                default_settings.default_download_folder;
                        }
                        ConfigItem::WatchFolder => {
                            settings_edit.watch_folder = default_settings.watch_folder;
                        }
                        ConfigItem::GlobalDownloadLimit => {
                            settings_edit.global_download_limit_bps =
                                default_settings.global_download_limit_bps;
                        }
                        ConfigItem::GlobalUploadLimit => {
                            settings_edit.global_upload_limit_bps =
                                default_settings.global_upload_limit_bps;
                        }
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let item = items[*selected_index];
                    let increment = 10_000 * 8;
                    match item {
                        ConfigItem::GlobalDownloadLimit => {
                            let new_rate =
                                settings_edit.global_download_limit_bps.saturating_add(increment);
                            settings_edit.global_download_limit_bps = new_rate;
                            let bucket = global_dl_bucket.clone();
                            tokio::spawn(async move {
                                bucket.set_rate(new_rate as f64);
                            });
                        }
                        ConfigItem::GlobalUploadLimit => {
                            let new_rate =
                                settings_edit.global_upload_limit_bps.saturating_add(increment);
                            settings_edit.global_upload_limit_bps = new_rate;
                            let bucket = global_ul_bucket.clone();
                            tokio::spawn(async move {
                                bucket.set_rate(new_rate as f64);
                            });
                        }
                        _ => {}
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    let item = items[*selected_index];
                    let decrement = 10_000 * 8;
                    match item {
                        ConfigItem::ClientPort => {}
                        ConfigItem::GlobalDownloadLimit => {
                            let new_rate =
                                settings_edit.global_download_limit_bps.saturating_sub(decrement);
                            settings_edit.global_download_limit_bps = new_rate;
                            let bucket = global_dl_bucket.clone();
                            tokio::spawn(async move {
                                bucket.set_rate(new_rate as f64);
                            });
                        }
                        ConfigItem::GlobalUploadLimit => {
                            let new_rate =
                                settings_edit.global_upload_limit_bps.saturating_sub(decrement);
                            settings_edit.global_upload_limit_bps = new_rate;
                            let bucket = global_ul_bucket.clone();
                            tokio::spawn(async move {
                                bucket.set_rate(new_rate as f64);
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    ConfigOutcome::Stay
}
