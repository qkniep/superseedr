// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::config::{PeerSortColumn, TorrentSortColumn};
use ratatui::prelude::*;

pub const MIN_SIDEBAR_WIDTH: u16 = 25;
pub const MIN_DETAILS_HEIGHT: u16 = 10;

#[derive(Default, Debug)]
pub struct FileBrowserLayout {
    pub area: Rect,
    pub content: Rect,
    pub footer: Rect,
    
    pub preview: Option<Rect>,
    pub browser: Rect,
    
    pub search: Option<Rect>,
    pub list: Rect,
    pub options: Option<Rect>, // Added: Area for Container Toggle/Input
}

// - Update calculate_file_browser_layout
pub fn calculate_file_browser_layout(
    area: Rect, 
    show_preview: bool, 
    show_search: bool,
    show_options: bool // Added parameter
) -> FileBrowserLayout {
    let mut plan = FileBrowserLayout::default();
    
    // 1. Global Split: Content vs Footer
    let main_chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ]).split(area);
    
    plan.area = area;
    plan.content = main_chunks[0];
    plan.footer = main_chunks[1];

    // 2. Horizontal Split: Preview vs Browser
    let content_chunks = if show_preview {
        Layout::horizontal([
            Constraint::Percentage(35), 
            Constraint::Percentage(65)
        ]).split(plan.content)
    } else {
        Layout::horizontal([
            Constraint::Percentage(0), 
            Constraint::Percentage(100)
        ]).split(plan.content)
    };

    if show_preview {
        plan.preview = Some(content_chunks[0]);
    }
    plan.browser = content_chunks[1];

    // 3. Browser Vertical Split: Search vs List vs Options
    let mut constraints = Vec::new();
    if show_search { constraints.push(Constraint::Length(3)); } // 0: Search
    constraints.push(Constraint::Min(0));                       // 1: List
    if show_options { constraints.push(Constraint::Length(3)); } // 2: Options

    let browser_chunks = Layout::vertical(constraints).split(plan.browser);

    let mut chunk_index = 0;
    if show_search {
        plan.search = Some(browser_chunks[chunk_index]);
        chunk_index += 1;
    }
    plan.list = browser_chunks[chunk_index];
    chunk_index += 1;
    
    if show_options {
        plan.options = Some(browser_chunks[chunk_index]);
    }

    plan
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnId {
    Status,
    Name,
    DownSpeed,
    UpSpeed,
}

pub struct ColumnDefinition {
    pub id: ColumnId,
    pub header: &'static str,
    pub min_width: u16,
    pub priority: u8,
    pub default_constraint: Constraint,
    pub sort_enum: Option<TorrentSortColumn>,
}

pub fn get_torrent_columns() -> Vec<ColumnDefinition> {
    vec![
        ColumnDefinition {
            id: ColumnId::Status,
            header: "Done",
            min_width: 7,
            priority: 2,
            default_constraint: Constraint::Length(7),
            sort_enum: Some(TorrentSortColumn::Progress),
        },
        ColumnDefinition {
            id: ColumnId::Name,
            header: "Name",
            min_width: 15,
            priority: 0,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(TorrentSortColumn::Name),
        },
        ColumnDefinition {
            id: ColumnId::UpSpeed,
            header: "UL",
            min_width: 10,
            priority: 1,
            default_constraint: Constraint::Length(10),
            sort_enum: Some(TorrentSortColumn::Up),
        },
        ColumnDefinition {
            id: ColumnId::DownSpeed,
            header: "DL",
            min_width: 10,
            priority: 1,
            default_constraint: Constraint::Length(10),
            sort_enum: Some(TorrentSortColumn::Down),
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PeerColumnId {
    Flags,
    Address,
    Client,
    Action,
    Progress,
    DownSpeed,
    UpSpeed,
}

pub struct PeerColumnDefinition {
    pub id: PeerColumnId,
    pub header: &'static str,
    pub min_width: u16,
    pub priority: u8,
    pub default_constraint: Constraint,
    pub sort_enum: Option<PeerSortColumn>,
}

pub fn get_peer_columns() -> Vec<PeerColumnDefinition> {
    vec![
        PeerColumnDefinition {
            id: PeerColumnId::Flags,
            header: "Flag",
            min_width: 4,
            priority: 1,
            default_constraint: Constraint::Length(4),
            sort_enum: Some(PeerSortColumn::Flags),
        },
        PeerColumnDefinition {
            id: PeerColumnId::Progress,
            header: "Status",
            min_width: 6,
            priority: 2,
            default_constraint: Constraint::Length(6),
            sort_enum: Some(PeerSortColumn::Completed),
        },
        PeerColumnDefinition {
            id: PeerColumnId::Address,
            header: "Address",
            min_width: 16,
            priority: 0,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::Address),
        },
        PeerColumnDefinition {
            id: PeerColumnId::UpSpeed,
            header: "Upload",
            min_width: 10,
            priority: 1,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::UL),
        },
        PeerColumnDefinition {
            id: PeerColumnId::DownSpeed,
            header: "Download",
            min_width: 10,
            priority: 1,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::DL),
        },
        PeerColumnDefinition {
            id: PeerColumnId::Client,
            header: "Client",
            min_width: 12,
            priority: 3,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::Client),
        },
        PeerColumnDefinition {
            id: PeerColumnId::Action,
            header: "Action",
            min_width: 12,
            priority: 5,
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::Action),
        },
    ]
}

#[derive(Clone, Debug)]
pub struct SmartCol {
    pub min_width: u16,
    pub priority: u8,
    pub constraint: Constraint,
}

pub fn compute_smart_table_layout(
    columns: &[SmartCol],
    available_width: u16,
    horizontal_padding: u16,
) -> (Vec<Constraint>, Vec<usize>) {
    let mut indexed_cols: Vec<(usize, &SmartCol)> = columns.iter().enumerate().collect();

    indexed_cols.sort_by(|a, b| a.1.priority.cmp(&b.1.priority).then(a.0.cmp(&b.0)));

    let mut active_indices = Vec::new();
    let mut current_used_width = 0;

    let expansion_reserve = if available_width < 80 {
        15
    } else if available_width < 140 {
        25
    } else {
        0
    };

    for (idx, col) in indexed_cols {
        let spacing_cost = if active_indices.is_empty() {
            0
        } else {
            horizontal_padding
        };

        if col.priority == 0 {
            active_indices.push(idx);
            current_used_width += col.min_width + spacing_cost;
        } else {
            let projected_width = current_used_width + col.min_width + spacing_cost;
            let effective_budget = available_width.saturating_sub(expansion_reserve);

            if projected_width <= effective_budget {
                active_indices.push(idx);
                current_used_width = projected_width;
            }
        }
    }

    active_indices.sort();

    let final_constraints = active_indices
        .iter()
        .map(|&i| columns[i].constraint)
        .collect();

    (final_constraints, active_indices)
}

#[derive(Default, Debug)]
pub struct LayoutPlan {
    pub list: Rect,
    pub footer: Rect,
    pub details: Rect,
    pub peers: Rect,
    pub chart: Option<Rect>,
    pub sparklines: Option<Rect>,
    pub stats: Option<Rect>,
    pub peer_stream: Option<Rect>,
    pub block_stream: Option<Rect>,
    pub warning_message: Option<String>,
}

pub struct LayoutContext {
    pub width: u16,
    pub height: u16,
    pub settings_sidebar_percent: u16,
}

impl LayoutContext {
    pub fn new(area: Rect, _app_state: &AppState, sidebar_pct: u16) -> Self {
        Self {
            width: area.width,
            height: area.height,
            settings_sidebar_percent: sidebar_pct,
        }
    }
}

pub fn calculate_layout(area: Rect, ctx: &LayoutContext) -> LayoutPlan {
    let mut plan = LayoutPlan::default();

    if ctx.width < 40 || ctx.height < 10 {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        plan.list = chunks[0];
        plan.footer = chunks[1];
        plan.warning_message = Some("Window too small".to_string());
        return plan;
    }

    let is_narrow = ctx.width < 100;
    let is_vertical_aspect = ctx.height as f32 > (ctx.width as f32 * 0.6);
    let is_short = ctx.height < 30;

    if is_short {
        let main = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(12),
            Constraint::Length(1),
        ])
        .split(area);

        let top_split =
            Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(main[0]);
        plan.list = top_split[0];
        plan.sparklines = Some(top_split[1]);

        let bottom_cols =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(main[1]);
        plan.stats = Some(bottom_cols[0]);

        let detail_chunks =
            Layout::vertical([Constraint::Length(9), Constraint::Length(0)]).split(bottom_cols[1]);
        plan.details = detail_chunks[0];
        plan.peers = detail_chunks[1];

        plan.footer = main[2];
    } else if is_narrow || is_vertical_aspect {
        let (chart_height, info_height) = if ctx.height < 50 {
            (10, MIN_DETAILS_HEIGHT)
        } else {
            (14, 20)
        };

        let v_chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(chart_height),
            Constraint::Length(info_height),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(area);

        if ctx.height < 70 {
            plan.list = v_chunks[0];
            plan.peer_stream = None;
        } else {
            let top_split =
                Layout::vertical([Constraint::Min(0), Constraint::Length(9)]).split(v_chunks[0]);

            plan.list = top_split[0];
            plan.peer_stream = Some(top_split[1]);
        }

        plan.chart = Some(v_chunks[1]);

        if ctx.width < 90 {
            let info_cols =
                Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).split(v_chunks[2]);

            let left_v =
                Layout::vertical([Constraint::Length(MIN_DETAILS_HEIGHT), Constraint::Min(0)])
                    .split(info_cols[0]);

            plan.details = left_v[0];
            plan.block_stream = Some(left_v[1]);
            plan.stats = Some(info_cols[1]);
        } else {
            let info_cols = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(14),
                Constraint::Fill(1),
            ])
            .split(v_chunks[2]);

            plan.details = info_cols[0];
            plan.block_stream = Some(info_cols[1]);
            plan.stats = Some(info_cols[2]);
        }

        plan.peers = v_chunks[3];
        plan.footer = v_chunks[4];
    } else {
        let main = Layout::vertical([
            Constraint::Min(10),
            Constraint::Length(27),
            Constraint::Length(1),
        ])
        .split(area);

        let top_area = main[0];
        let bottom_area = main[1];
        plan.footer = main[2];

        let target_sidebar =
            (ctx.width as f32 * (ctx.settings_sidebar_percent as f32 / 100.0)) as u16;
        let sidebar_width = target_sidebar.max(MIN_SIDEBAR_WIDTH);

        let top_h = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)])
            .split(top_area);

        let left_v = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(top_h[0]);
        plan.list = left_v[0];
        plan.sparklines = Some(left_v[1]);

        let right_v = Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).split(top_h[1]);

        let header_h =
            Layout::horizontal([Constraint::Length(40), Constraint::Min(0)]).split(right_v[0]);

        plan.details = header_h[0];
        plan.peer_stream = Some(header_h[1]);
        plan.peers = right_v[1];

        let show_block_stream = ctx.width > 135;
        let right_pane_width = if show_block_stream { 54 } else { 40 };

        let bottom_h =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_pane_width)])
                .split(bottom_area);

        plan.chart = Some(bottom_h[0]);
        let stats_area = bottom_h[1];

        if show_block_stream {
            let stats_h =
                Layout::horizontal([Constraint::Length(14), Constraint::Min(0)]).split(stats_area);

            plan.block_stream = Some(stats_h[0]);
            plan.stats = Some(stats_h[1]);
        } else {
            plan.stats = Some(stats_area);
            plan.block_stream = None;
        }
    }

    plan
}
