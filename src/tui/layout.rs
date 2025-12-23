// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::config::{PeerSortColumn, TorrentSortColumn};
use ratatui::prelude::*;

// --- 1. SHARED CONSTANTS (The Contract) ---
pub const MIN_SIDEBAR_WIDTH: u16 = 25;
pub const MIN_CHART_HEIGHT: u16 = 8;
pub const MIN_DETAILS_HEIGHT: u16 = 10;

// --- TORRENT COLUMNS ---

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnId {
    Status, // e.g. "Done" or Progress
    Name,
    DownSpeed,
    UpSpeed,
    // Add others easily later: Peers, ETA, etc.
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
            priority: 2, // Low priority
            default_constraint: Constraint::Length(7),
            sort_enum: Some(TorrentSortColumn::Progress),
        },
        ColumnDefinition {
            id: ColumnId::Name,
            header: "Name",
            min_width: 15,
            priority: 0, // Essential
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(TorrentSortColumn::Name),
        },
        ColumnDefinition {
            id: ColumnId::UpSpeed,
            header: "UL",
            min_width: 10,
            priority: 1, // Medium
            default_constraint: Constraint::Length(10),
            sort_enum: Some(TorrentSortColumn::Up),
        },
        ColumnDefinition {
            id: ColumnId::DownSpeed,
            header: "DL",
            min_width: 10,
            priority: 1, // Medium
            default_constraint: Constraint::Length(10),
            sort_enum: Some(TorrentSortColumn::Down),
        },
    ]
}

// --- PEER COLUMNS ---

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

/// Defines the layout for the Peers Table.
/// Priorities are set to drop columns aggressively on small screens.
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
            priority: 0, // Essential
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::Address),
        },
        PeerColumnDefinition {
            id: PeerColumnId::UpSpeed,
            header: "Upload",
            min_width: 10,
            priority: 1, // Keep speeds if possible
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::UL),
        },
        PeerColumnDefinition {
            id: PeerColumnId::DownSpeed,
            header: "Download",
            min_width: 10,
            priority: 1, // Keep speeds if possible
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
            priority: 5, // Drop early
            default_constraint: Constraint::Fill(1),
            sort_enum: Some(PeerSortColumn::Action),
        },
    ]
}

// --- 2. SMART TABLE GENERATOR ---

#[derive(Clone, Debug)]
pub struct SmartCol<'a> {
    pub header: &'a str,
    pub min_width: u16,
    pub priority: u8, // 0 = Always show (Essential), 1+ = Optional
    pub constraint: Constraint,
}

/// Calculates which columns fit into the given width.
///
/// **Aggressive Hiding Logic:**
/// If a column is "Optional" (Priority > 0), we check if adding it would
/// infringe on the "Breathing Room" of the main Priority 0 column.
/// This ensures the 'Name' or 'Address' column always has room to expand
/// beyond its minimum width.
pub fn compute_smart_table_layout(
    columns: &[SmartCol],
    available_width: u16,
    horizontal_padding: u16,
) -> (Vec<Constraint>, Vec<usize>) {
    let mut indexed_cols: Vec<(usize, &SmartCol)> = columns.iter().enumerate().collect();

    // Sort by Priority (Low numbers = High priority), then by original index
    indexed_cols.sort_by(|a, b| a.1.priority.cmp(&b.1.priority).then(a.0.cmp(&b.0)));

    let mut active_indices = Vec::new();
    let mut current_used_width = 0;

    // Calculate an "Expansion Reserve".
    // If the screen is narrow (<120), we demand that the Priority 0 column
    // gets at least 20-30% extra space, effectively reducing the budget for optional columns.
    let expansion_reserve = if available_width < 80 {
        15 // Very narrow: reserve modest space so names aren't 100% truncated
    } else if available_width < 140 {
        25 // Standard narrow: ensure good readability for names
    } else {
        0 // Wide screen: pack everything in
    };

    // Greedily pick columns that fit
    for (idx, col) in indexed_cols {
        let spacing_cost = if active_indices.is_empty() {
            0
        } else {
            horizontal_padding
        };

        if col.priority == 0 {
            // Essential: Always add it, but track its min cost
            active_indices.push(idx);
            current_used_width += col.min_width + spacing_cost;
        } else {
            // Optional: Check fit against Adjusted Budget (Available - Reserve)
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

// --- 3. LAYOUT PLAN DEFINITION ---

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
    pub has_chart_data: bool,
}

impl LayoutContext {
    pub fn new(area: Rect, app_state: &AppState, sidebar_pct: u16) -> Self {
        let has_data = !app_state.avg_download_history.is_empty();
        Self {
            width: area.width,
            height: area.height,
            settings_sidebar_percent: sidebar_pct,
            has_chart_data: has_data,
        }
    }
}

// --- 4. THE CALCULATOR FUNCTION ---

pub fn calculate_layout(area: Rect, ctx: &LayoutContext) -> LayoutPlan {
    let mut plan = LayoutPlan::default();

    // 1. SAFETY CHECKS (Tiny Screens)
    if ctx.width < 40 || ctx.height < 10 {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        plan.list = chunks[0];
        plan.footer = chunks[1];
        plan.warning_message = Some("Window too small".to_string());
        return plan;
    }

    // 2. ASPECT RATIO / MODE CHECK
    let is_narrow = ctx.width < 100;
    let is_vertical_aspect = ctx.height as f32 > (ctx.width as f32 * 0.6);
    let is_short = ctx.height < 30; // Compact mode check

    if is_short {
        // --- COMPACT MODE ---
        let main = Layout::vertical([
            Constraint::Min(5),     // List/Sparklines
            Constraint::Length(12), // Details/Stats
            Constraint::Length(1),  // Footer
        ])
        .split(area);

        // Top: List vs Sparklines
        let top_split =
            Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(main[0]);
        plan.list = top_split[0];
        plan.sparklines = Some(top_split[1]);

        // Bottom: Stats vs Details
        let bottom_cols =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(main[1]);
        plan.stats = Some(bottom_cols[0]);

        // Details column split
        let detail_chunks =
            Layout::vertical([Constraint::Length(9), Constraint::Length(0)]).split(bottom_cols[1]);
        plan.details = detail_chunks[0];
        plan.peers = detail_chunks[1]; // Hidden in compact

        plan.footer = main[2];
    } else if is_narrow || is_vertical_aspect {
        // --- PORTRAIT MODE ---

        // DYNAMIC SIZING:
        // If height is < 50, we compress the Chart and Info rows to ensure
        // the Torrent List and Peers Table (the flexible Fill regions)
        // don't collapse to 0 height.
        // Total Fixed Cost (Compressed): 10 (Chart) + 10 (Info) + 1 (Footer) = 21 Rows.
        // At height 30, this leaves 9 rows for List/Peers.

        let (chart_height, info_height) = if ctx.height < 50 {
            (10, MIN_DETAILS_HEIGHT) // Height < 50: Compressed Chart (10) & Compact Details (10)
        } else {
            (14, 20) // Height >= 50: Full Chart (14) & Expanded Details (20)
        };

        let v_chunks = Layout::vertical([
            Constraint::Fill(1), // List
            Constraint::Length(chart_height),
            Constraint::Length(info_height),
            Constraint::Fill(1),   // Peers Table
            Constraint::Length(1), // Footer
        ])
        .split(area);

        // 1. Top Section
        if ctx.height < 70 {
            // HEIGHT CONSTRAINED: Hide Peer Stream
            plan.list = v_chunks[0];
            plan.peer_stream = None;
        } else {
            // HEIGHT SUFFICIENT: Show both
            let top_split = Layout::vertical([
                Constraint::Min(0),    // List
                Constraint::Length(9), // Peer Stream
            ])
            .split(v_chunks[0]);

            plan.list = top_split[0];
            plan.peer_stream = Some(top_split[1]);
        }

        // 2. Chart (Always shown, just compressed)
        plan.chart = Some(v_chunks[1]);

        // 3. Info Row (MODIFIED)
        // If width is tight (< 90), stack Details & BlockStream vertically
        // to give Details more breathing room.
        if ctx.width < 90 {
            let info_cols = Layout::horizontal([
                Constraint::Fill(1), // Left Col: Details + BlockStream
                Constraint::Fill(1), // Right Col: Stats
            ])
            .split(v_chunks[2]);

            // Split Left Col vertically
            let left_v = Layout::vertical([
                Constraint::Length(MIN_DETAILS_HEIGHT), // Details on top
                Constraint::Min(0),                     // BlockStream below
            ])
            .split(info_cols[0]);

            plan.details = left_v[0];
            plan.block_stream = Some(left_v[1]);
            plan.stats = Some(info_cols[1]);
        } else {
            // Standard: 3 Columns side-by-side
            let info_cols = Layout::horizontal([
                Constraint::Fill(1),    // Details Text
                Constraint::Length(14), // Block Stream
                Constraint::Fill(1),    // Stats
            ])
            .split(v_chunks[2]);

            plan.details = info_cols[0];
            plan.block_stream = Some(info_cols[1]);
            plan.stats = Some(info_cols[2]);
        }

        // 4. Peers Table
        plan.peers = v_chunks[3];

        // 5. Footer
        plan.footer = v_chunks[4];
    } else {
        // --- LANDSCAPE MODE ---

        let main = Layout::vertical([
            Constraint::Min(10),    // Top Area
            Constraint::Length(27), // Bottom Area
            Constraint::Length(1),  // Footer
        ])
        .split(area);

        let top_area = main[0];
        let bottom_area = main[1];
        plan.footer = main[2];

        // Top Horizontal Split
        let target_sidebar =
            (ctx.width as f32 * (ctx.settings_sidebar_percent as f32 / 100.0)) as u16;
        let sidebar_width = target_sidebar.max(MIN_SIDEBAR_WIDTH);

        let top_h = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)])
            .split(top_area);

        // Left Pane
        let left_v = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(top_h[0]);
        plan.list = left_v[0];
        plan.sparklines = Some(left_v[1]);

        // Right Pane
        let right_v = Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).split(top_h[1]);

        // Header
        let header_h =
            Layout::horizontal([Constraint::Length(40), Constraint::Min(0)]).split(right_v[0]);

        plan.details = header_h[0];
        plan.peer_stream = Some(header_h[1]);
        plan.peers = right_v[1];

        // --- BOTTOM AREA LOGIC ---
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
