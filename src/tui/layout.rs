// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::prelude::*;
use crate::app::AppState;

// --- 1. SHARED CONSTANTS (The Contract) ---
pub const MIN_SIDEBAR_WIDTH: u16 = 25;
pub const MIN_CHART_HEIGHT: u16 = 8;
pub const MIN_DETAILS_HEIGHT: u16 = 10;

// --- 2. SMART TABLE GENERATOR ---

#[derive(Clone, Debug)]
pub struct SmartCol<'a> {
    pub header: &'a str,
    pub min_width: u16,
    pub priority: u8, // 0 = Always show, 1+ = Drop if needed
    pub constraint: Constraint,
}

/// Calculates which columns fit into the given width.
pub fn compute_smart_table_layout(
    columns: &[SmartCol],
    available_width: u16,
    horizontal_padding: u16,
) -> (Vec<Constraint>, Vec<usize>) {
    let mut indexed_cols: Vec<(usize, &SmartCol)> = columns.iter().enumerate().collect();

    // Sort by Priority (Low numbers = High priority), then by original index
    indexed_cols.sort_by(|a, b| {
        a.1.priority.cmp(&b.1.priority).then(a.0.cmp(&b.0))
    });

    let mut active_indices = Vec::new();
    let mut current_used_width = 0;

    // Greedily pick columns that fit
    for (idx, col) in indexed_cols {
        let spacing_cost = if active_indices.is_empty() { 0 } else { horizontal_padding };
        let projected_width = current_used_width + col.min_width + spacing_cost;

        if col.priority == 0 || projected_width <= available_width {
            active_indices.push(idx);
            current_used_width = projected_width;
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
    // -- Core Areas --
    pub list: Rect,
    pub footer: Rect,

    // -- Details Split --
    // We split details into 'Text' (info) and 'Peers' (table) because 
    // in Portrait mode they are in completely different places.
    pub details: Rect,      // The text info pane
    pub peers: Rect,        // The peer table/heatmap

    // -- Optional Areas --
    pub chart: Option<Rect>,
    pub sparklines: Option<Rect>,
    pub stats: Option<Rect>,
    pub peer_stream: Option<Rect>,
    pub block_stream: Option<Rect>,
    
    // -- Overlays --
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
        // In tiny mode, we might map peers/details to width=0 area just to be safe, 
        // or handle Option in draw. For now, they default to empty Rects (0,0,0,0).
        return plan;
    }

    // 2. ASPECT RATIO / MODE CHECK
    // Logic from tui.rs: "If width < 100 OR height > width * 0.6 -> Portrait"
    let is_narrow = ctx.width < 100;
    let is_vertical_aspect = ctx.height as f32 > (ctx.width as f32 * 0.6);
    let is_short = ctx.height < 30; // Compact mode check

    if is_short {
        // --- COMPACT MODE ---
        // (Simplified from draw_compact_layout in tui.rs)
        let main = Layout::vertical([
            Constraint::Min(5),     // List/Sparklines
            Constraint::Length(12), // Details/Stats
            Constraint::Length(1),  // Footer
        ]).split(area);

        // Top: List vs Sparklines
        let top_split = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(main[0]);
        plan.list = top_split[0];
        plan.sparklines = Some(top_split[1]);

        // Bottom: Stats vs Details
        let bottom_cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(main[1]);
        plan.stats = Some(bottom_cols[0]);

        // Details column split (Text vs Peers Table - Peers hidden in compact usually)
        let detail_chunks = Layout::vertical([Constraint::Length(9), Constraint::Length(0)]).split(bottom_cols[1]);
        plan.details = detail_chunks[0];
        plan.peers = detail_chunks[1]; // Likely empty/hidden

        plan.footer = main[2];

    } else if is_narrow || is_vertical_aspect {
        // --- PORTRAIT MODE ---
        // (Replicating draw_portrait_layout from tui.rs lines 184-236)
        
        let v_chunks = Layout::vertical([
            Constraint::Fill(1),        // List + Peer Stream
            Constraint::Length(14),     // Chart
            Constraint::Length(20),     // Info Row (Details | BlockStream | Stats)
            Constraint::Fill(1),        // Peers Table (Bottom)
            Constraint::Length(1),      // Footer
        ]).split(area);

        // 1. Top Section (List vs Peer Stream)
        let top_split = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(10),
        ]).split(v_chunks[0]);
        
        plan.list = top_split[0];
        plan.peer_stream = Some(top_split[1]);

        // 2. Chart
        plan.chart = Some(v_chunks[1]);

        // 3. Info Row (Details | Block Stream | Stats)
        let info_cols = Layout::horizontal([
            Constraint::Fill(1),        // Details Text
            Constraint::Length(14),     // Block Stream (Fixed)
            Constraint::Fill(1),        // Stats
        ]).split(v_chunks[2]);

        plan.details = info_cols[0];
        plan.block_stream = Some(info_cols[1]);
        plan.stats = Some(info_cols[2]);

        // 4. Peers Table (The large bottom area)
        plan.peers = v_chunks[3];

        // 5. Footer
        plan.footer = v_chunks[4];

    } else {
        // --- LANDSCAPE MODE ---
        // (Replicating draw_landscape_layout from tui.rs lines 140-182)

        let main = Layout::vertical([
            Constraint::Min(10),    // Top Area
            Constraint::Length(27), // Bottom Area
            Constraint::Length(1),  // Footer
        ]).split(area);

        let top_area = main[0];
        let bottom_area = main[1];
        plan.footer = main[2];

        // Top Horizontal Split (Sidebar vs Main)
        let target_sidebar = (ctx.width as f32 * (ctx.settings_sidebar_percent as f32 / 100.0)) as u16;
        let sidebar_width = target_sidebar.max(MIN_SIDEBAR_WIDTH);
        
        let top_h = Layout::horizontal([
            Constraint::Length(sidebar_width),
            Constraint::Min(0)
        ]).split(top_area);

        // Left Pane: List vs Sparklines
        let left_v = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(top_h[0]);
        plan.list = left_v[0];
        plan.sparklines = Some(left_v[1]);

        // Right Pane: Details Header vs Peers Table
        // tui.rs: right_pane_chunks = Vertical [Length(9), Min(0)]
        let right_v = Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).split(top_h[1]);
        
        // The "Header" area (right_v[0]) contains Details Text AND Peer Stream
        let header_h = Layout::horizontal([Constraint::Percentage(20), Constraint::Percentage(80)]).split(right_v[0]);
        
        plan.details = header_h[0];     // Text Details
        plan.peer_stream = Some(header_h[1]); // Peer Stream (Dots)
        plan.peers = right_v[1];        // Peer Table (Bottom of right pane)

        // Bottom Area: Chart vs Stats/BlockStream
        let bottom_h = Layout::horizontal([Constraint::Percentage(77), Constraint::Percentage(23)]).split(bottom_area);
        
        plan.chart = Some(bottom_h[0]);

        // Stats Area Split: Stats vs Block Stream
        let stats_h = Layout::horizontal([Constraint::Min(0), Constraint::Length(14)]).split(bottom_h[1]);
        plan.stats = Some(stats_h[0]);
        plan.block_stream = Some(stats_h[1]);
    }

    plan
}
