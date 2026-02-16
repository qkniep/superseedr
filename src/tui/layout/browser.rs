// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::BrowserPane;
use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Default, Debug)]
pub struct FileBrowserLayout {
    pub area: Rect,
    pub content: Rect,
    pub footer: Rect,

    pub preview: Option<Rect>,
    pub browser: Rect,

    pub search: Option<Rect>,
    pub list: Rect,
}

pub fn calculate_file_browser_layout(
    area: Rect,
    show_preview: bool,
    show_search: bool,
    focused_pane: &BrowserPane,
) -> FileBrowserLayout {
    let mut plan = FileBrowserLayout::default();
    let main_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);

    plan.area = area;
    plan.content = main_chunks[0];
    plan.footer = main_chunks[1];

    let is_narrow = area.width < 100 || (area.height as f32 > (area.width as f32 * 0.6));

    let content_chunks = if show_preview {
        if is_narrow {
            let constraints = match focused_pane {
                BrowserPane::FileSystem => [Constraint::Percentage(35), Constraint::Percentage(65)],
                BrowserPane::TorrentPreview => {
                    [Constraint::Percentage(60), Constraint::Percentage(40)]
                }
            };
            Layout::vertical(constraints).split(plan.content)
        } else {
            let constraints = match focused_pane {
                BrowserPane::FileSystem => [Constraint::Percentage(35), Constraint::Percentage(65)],
                BrowserPane::TorrentPreview => {
                    [Constraint::Percentage(60), Constraint::Percentage(40)]
                }
            };
            Layout::horizontal(constraints).split(plan.content)
        }
    } else {
        Layout::horizontal([Constraint::Percentage(0), Constraint::Percentage(100)])
            .split(plan.content)
    };

    plan.preview = if show_preview {
        Some(content_chunks[0])
    } else {
        None
    };
    plan.browser = content_chunks[1];

    let browser_chunks = if show_search {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(plan.browser)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(plan.browser)
    };

    plan.search = if show_search {
        Some(browser_chunks[0])
    } else {
        None
    };
    plan.list = if show_search {
        browser_chunks[1]
    } else {
        browser_chunks[0]
    };

    plan
}
