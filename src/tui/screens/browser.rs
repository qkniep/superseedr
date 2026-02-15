// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::{AppCommand, AppMode, BrowserPane, ConfigItem, FileBrowserMode, FileMetadata};
use crate::tui::formatters::centered_rect;
use crate::tui::layout::calculate_file_browser_layout;
use crate::app::TorrentPreviewPayload;
use crate::tui::tree::{RawNode, TreeAction, TreeFilter, TreeMathHelper, TreeViewState};
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

pub fn handle_search_interceptor(
    key_code: KeyCode,
    is_searching: &mut bool,
    search_query: &mut String,
) -> bool {
    if !*is_searching {
        return false;
    }

    match key_code {
        KeyCode::Esc => {
            *is_searching = false;
            search_query.clear();
        }
        KeyCode::Enter => {
            *is_searching = false;
        }
        KeyCode::Backspace => {
            search_query.pop();
        }
        KeyCode::Char(c) => {
            search_query.push(c);
        }
        _ => {}
    }

    true
}

pub fn handle_download_name_edit_guard(key_code: KeyCode, browser_mode: &mut FileBrowserMode) -> bool {
    if let FileBrowserMode::DownloadLocSelection {
        container_name,
        is_editing_name,
        cursor_pos,
        original_name_backup,
        ..
    } = browser_mode
    {
        if !*is_editing_name {
            return false;
        }

        match key_code {
            KeyCode::Enter => {
                *is_editing_name = false;
            }
            KeyCode::Esc => {
                *container_name = original_name_backup.clone();
                *is_editing_name = false;
                *cursor_pos = container_name.len();
            }
            KeyCode::Left => {
                *cursor_pos = cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                if *cursor_pos < container_name.len() {
                    *cursor_pos += 1;
                }
            }
            KeyCode::Backspace => {
                if *cursor_pos > 0 {
                    container_name.remove(*cursor_pos - 1);
                    *cursor_pos -= 1;
                }
            }
            KeyCode::Delete => {
                if *cursor_pos < container_name.len() {
                    container_name.remove(*cursor_pos);
                }
            }
            KeyCode::Char(c) => {
                container_name.insert(*cursor_pos, c);
                *cursor_pos += 1;
            }
            _ => {}
        }

        return true;
    }

    false
}

pub fn handle_download_shortcuts(key_code: KeyCode, browser_mode: &mut FileBrowserMode) -> bool {
    if let FileBrowserMode::DownloadLocSelection {
        container_name,
        use_container,
        is_editing_name,
        focused_pane,
        cursor_pos,
        original_name_backup,
        ..
    } = browser_mode
    {
        match key_code {
            KeyCode::Char('x') => {
                *use_container = !*use_container;
                true
            }
            KeyCode::Char('r') if *use_container => {
                *is_editing_name = true;
                *original_name_backup = container_name.clone();
                *cursor_pos = container_name.len();
                *focused_pane = BrowserPane::TorrentPreview;
                true
            }
            KeyCode::Tab => {
                *focused_pane = match focused_pane {
                    BrowserPane::FileSystem => BrowserPane::TorrentPreview,
                    BrowserPane::TorrentPreview => BrowserPane::FileSystem,
                };
                true
            }
            _ => false,
        }
    } else {
        false
    }
}

pub fn has_preview_content(
    browser_mode: &FileBrowserMode,
    pending_torrent_path: bool,
    pending_torrent_link: bool,
    cursor_path: Option<&std::path::PathBuf>,
) -> bool {
    match browser_mode {
        FileBrowserMode::DownloadLocSelection { .. } => pending_torrent_path || pending_torrent_link,
        FileBrowserMode::File(_) => {
            cursor_path.is_some_and(|p| p.extension().is_some_and(|ext| ext == "torrent"))
        }
        _ => false,
    }
}

pub fn focused_pane(browser_mode: &FileBrowserMode) -> BrowserPane {
    if let FileBrowserMode::DownloadLocSelection { focused_pane, .. } = browser_mode {
        focused_pane.clone()
    } else {
        BrowserPane::FileSystem
    }
}

pub fn calculate_area(screen: Rect, has_preview_content: bool) -> Rect {
    if has_preview_content {
        if screen.width < 60 {
            screen
        } else {
            centered_rect(90, 80, screen)
        }
    } else if screen.width < 40 {
        screen
    } else {
        centered_rect(75, 80, screen)
    }
}

pub fn calculate_list_height(
    screen: Rect,
    has_preview_content: bool,
    is_searching: bool,
    focused_pane: &BrowserPane,
) -> usize {
    let area = calculate_area(screen, has_preview_content);
    let layout = calculate_file_browser_layout(area, has_preview_content, is_searching, focused_pane);
    layout.list.height.saturating_sub(2) as usize
}

pub fn calculate_preview_list_height(
    screen: Rect,
    is_searching: bool,
    focused_pane: &BrowserPane,
    use_container: bool,
) -> Option<usize> {
    let area = if screen.width < 60 {
        screen
    } else {
        centered_rect(90, 80, screen)
    };
    let layout = calculate_file_browser_layout(area, true, is_searching, focused_pane);
    layout.preview.map(|preview_rect| {
        let inner_height = preview_rect.height.saturating_sub(2);
        let header_rows = if use_container { 2 } else { 1 };
        inner_height.saturating_sub(header_rows) as usize
    })
}

pub fn apply_preview_navigation(
    key_code: KeyCode,
    preview_state: &mut TreeViewState,
    preview_tree: &mut [RawNode<TorrentPreviewPayload>],
    list_height: usize,
) -> bool {
    let action = match key_code {
        KeyCode::Up | KeyCode::Char('k') => Some(TreeAction::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(TreeAction::Down),
        KeyCode::Left | KeyCode::Char('h') => Some(TreeAction::Left),
        KeyCode::Right | KeyCode::Char('l') => Some(TreeAction::Right),
        _ => None,
    };

    if let Some(action) = action {
        TreeMathHelper::apply_action(
            preview_state,
            preview_tree,
            action,
            TreeFilter::default(),
            list_height,
        );
        true
    } else {
        false
    }
}

pub fn build_filter(browser_mode: &FileBrowserMode, search_query: &str) -> TreeFilter<FileMetadata> {
    match browser_mode {
        FileBrowserMode::Directory
        | FileBrowserMode::DownloadLocSelection { .. }
        | FileBrowserMode::ConfigPathSelection { .. } => TreeFilter::from_text(search_query),
        FileBrowserMode::File(extensions) => {
            let exts = extensions.clone();
            TreeFilter::new(search_query, move |node| {
                node.is_dir || exts.iter().any(|ext| node.name.ends_with(ext))
            })
        }
    }
}

pub fn handle_filesystem_navigation(
    key_code: KeyCode,
    state: &mut TreeViewState,
    data: &[RawNode<FileMetadata>],
    browser_mode: &FileBrowserMode,
    is_searching: &mut bool,
    search_query: &mut String,
    list_height: usize,
    app_command_tx: &mpsc::Sender<AppCommand>,
) -> bool {
    let filter = build_filter(browser_mode, search_query);

    match key_code {
        KeyCode::Char('/') => {
            *is_searching = true;
            search_query.clear();
            true
        }
        KeyCode::Up | KeyCode::Char('k') => {
            TreeMathHelper::apply_action(state, data, TreeAction::Up, filter, list_height);
            true
        }
        KeyCode::Down | KeyCode::Char('j') => {
            TreeMathHelper::apply_action(state, data, TreeAction::Down, filter, list_height);
            true
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            if let Some(path) = state.cursor_path.clone() {
                if path.is_dir() {
                    *is_searching = false;
                    search_query.clear();
                    let _ = app_command_tx.try_send(AppCommand::FetchFileTree {
                        path,
                        browser_mode: browser_mode.clone(),
                        highlight_path: None,
                    });
                }
            }
            true
        }
        KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('u') => {
            let child_to_highlight = state.current_path.clone();
            if let Some(parent) = state.current_path.parent() {
                let _ = app_command_tx.try_send(AppCommand::FetchFileTree {
                    path: parent.to_path_buf(),
                    browser_mode: browser_mode.clone(),
                    highlight_path: Some(child_to_highlight),
                });
            }
            true
        }
        _ => false,
    }
}

pub fn confirm_config_path_selection(
    state: &TreeViewState,
    browser_mode: &FileBrowserMode,
) -> Option<AppMode> {
    if let FileBrowserMode::ConfigPathSelection {
        target_item,
        current_settings,
        selected_index,
        items,
    } = browser_mode
    {
        let mut new_settings = current_settings.clone();
        let selected_path = state.current_path.clone();

        match target_item {
            ConfigItem::DefaultDownloadFolder => new_settings.default_download_folder = Some(selected_path),
            ConfigItem::WatchFolder => new_settings.watch_folder = Some(selected_path),
            _ => {}
        }

        return Some(AppMode::Config {
            settings_edit: new_settings,
            selected_index: *selected_index,
            items: items.clone(),
            editing: None,
        });
    }
    None
}

pub fn escape_to_config_mode(browser_mode: &FileBrowserMode) -> Option<AppMode> {
    if let FileBrowserMode::ConfigPathSelection {
        current_settings,
        selected_index,
        items,
        ..
    } = browser_mode
    {
        return Some(AppMode::Config {
            settings_edit: current_settings.clone(),
            selected_index: *selected_index,
            items: items.clone(),
            editing: None,
        });
    }
    None
}

pub fn selected_torrent_file_for_confirm(
    state: &TreeViewState,
    browser_mode: &FileBrowserMode,
) -> Option<std::path::PathBuf> {
    if let FileBrowserMode::File(extensions) = browser_mode {
        if let Some(path) = state.cursor_path.clone() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if extensions.iter().any(|ext| name.ends_with(ext)) {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{BrowserPane, ConfigItem, TorrentPreviewPayload};
    use crate::tui::tree::{RawNode, TreeViewState};
    use std::path::PathBuf;

    #[test]
    fn search_interceptor_clears_on_escape() {
        let mut is_searching = true;
        let mut query = String::from("abc");
        let consumed = handle_search_interceptor(KeyCode::Esc, &mut is_searching, &mut query);
        assert!(consumed);
        assert!(!is_searching);
        assert!(query.is_empty());
    }

    #[test]
    fn name_edit_guard_updates_buffer_and_cursor() {
        let mut mode = FileBrowserMode::DownloadLocSelection {
            torrent_files: vec![],
            container_name: "ab".to_string(),
            use_container: true,
            is_editing_name: true,
            focused_pane: BrowserPane::TorrentPreview,
            preview_tree: vec![RawNode {
                name: "x".to_string(),
                full_path: PathBuf::from("x"),
                children: vec![],
                payload: TorrentPreviewPayload::default(),
                is_dir: false,
            }],
            preview_state: TreeViewState::default(),
            cursor_pos: 2,
            original_name_backup: "ab".to_string(),
        };

        let consumed = handle_download_name_edit_guard(KeyCode::Char('c'), &mut mode);
        assert!(consumed);
        match mode {
            FileBrowserMode::DownloadLocSelection {
                container_name,
                cursor_pos,
                ..
            } => {
                assert_eq!(container_name, "abc");
                assert_eq!(cursor_pos, 3);
            }
            _ => panic!("expected DownloadLocSelection"),
        }
    }

    #[test]
    fn name_edit_guard_ignored_when_not_editing() {
        let mut mode = FileBrowserMode::ConfigPathSelection {
            target_item: ConfigItem::WatchFolder,
            current_settings: Box::default(),
            selected_index: 0,
            items: vec![],
        };
        let consumed = handle_download_name_edit_guard(KeyCode::Char('x'), &mut mode);
        assert!(!consumed);
    }

    #[test]
    fn download_shortcuts_toggle_pane() {
        let mut mode = FileBrowserMode::DownloadLocSelection {
            torrent_files: vec![],
            container_name: "x".to_string(),
            use_container: true,
            is_editing_name: false,
            focused_pane: BrowserPane::FileSystem,
            preview_tree: vec![],
            preview_state: TreeViewState::default(),
            cursor_pos: 1,
            original_name_backup: "x".to_string(),
        };
        let consumed = handle_download_shortcuts(KeyCode::Tab, &mut mode);
        assert!(consumed);
        match mode {
            FileBrowserMode::DownloadLocSelection { focused_pane, .. } => {
                assert_eq!(focused_pane, BrowserPane::TorrentPreview);
            }
            _ => panic!("expected DownloadLocSelection"),
        }
    }

    #[test]
    fn has_preview_content_matches_file_mode_torrent_extension() {
        let mode = FileBrowserMode::File(vec![".torrent".to_string()]);
        let path = PathBuf::from("demo.torrent");
        assert!(has_preview_content(&mode, false, false, Some(&path)));
    }

    #[test]
    fn preview_navigation_consumes_direction_key() {
        let mut tree = vec![RawNode {
            name: "root".to_string(),
            full_path: PathBuf::from("root"),
            children: vec![RawNode {
                name: "child".to_string(),
                full_path: PathBuf::from("root/child"),
                children: vec![],
                payload: TorrentPreviewPayload::default(),
                is_dir: false,
            }],
            payload: TorrentPreviewPayload::default(),
            is_dir: true,
        }];
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("root"));
        state.cursor_path = Some(PathBuf::from("root"));
        assert!(apply_preview_navigation(
            KeyCode::Down,
            &mut state,
            &mut tree,
            10
        ));
    }

    #[test]
    fn filesystem_navigation_starts_search() {
        let mut state = TreeViewState::default();
        let data: Vec<RawNode<FileMetadata>> = vec![];
        let mode = FileBrowserMode::Directory;
        let (tx, _rx) = mpsc::channel(1);
        let mut is_searching = false;
        let mut query = String::from("abc");
        let consumed = handle_filesystem_navigation(
            KeyCode::Char('/'),
            &mut state,
            &data,
            &mode,
            &mut is_searching,
            &mut query,
            5,
            &tx,
        );
        assert!(consumed);
        assert!(is_searching);
        assert!(query.is_empty());
    }

    #[test]
    fn confirm_config_path_selection_returns_config_mode() {
        let mode = FileBrowserMode::ConfigPathSelection {
            target_item: ConfigItem::WatchFolder,
            current_settings: Box::default(),
            selected_index: 2,
            items: vec![ConfigItem::WatchFolder],
        };
        let state = TreeViewState {
            current_path: PathBuf::from("/tmp"),
            ..Default::default()
        };
        let out = confirm_config_path_selection(&state, &mode);
        assert!(matches!(out, Some(AppMode::Config { .. })));
    }
}
