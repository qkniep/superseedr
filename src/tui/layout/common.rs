// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::torrent_is_effectively_incomplete;
use crate::app::AppState;
use crate::config::{PeerSortColumn, TorrentSortColumn};
use ratatui::prelude::Constraint;

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

pub fn torrent_has_download_activity(app_state: &AppState) -> bool {
    app_state
        .torrents
        .values()
        .any(|t| t.smoothed_download_speed_bps > 0)
}

pub fn torrent_has_upload_activity(app_state: &AppState) -> bool {
    app_state
        .torrents
        .values()
        .any(|t| t.smoothed_upload_speed_bps > 0)
}

pub fn has_incomplete_torrents(app_state: &AppState) -> bool {
    app_state
        .torrents
        .values()
        .any(|t| torrent_is_effectively_incomplete(&t.latest_state))
}

pub fn active_torrent_column_indices(app_state: &AppState) -> Vec<usize> {
    let has_dl_activity = torrent_has_download_activity(app_state);
    let has_ul_activity = torrent_has_upload_activity(app_state);
    let has_incomplete = has_incomplete_torrents(app_state);

    get_torrent_columns()
        .iter()
        .enumerate()
        .filter_map(|(idx, col)| {
            let is_active = match col.id {
                ColumnId::DownSpeed => has_dl_activity,
                ColumnId::UpSpeed => has_ul_activity,
                ColumnId::Status => has_incomplete,
                ColumnId::Name => true,
            };
            is_active.then_some(idx)
        })
        .collect()
}

pub fn compute_visible_torrent_columns(
    app_state: &AppState,
    available_width: u16,
) -> (Vec<Constraint>, Vec<usize>) {
    let all_cols = get_torrent_columns();
    let active_indices = active_torrent_column_indices(app_state);

    let smart_cols: Vec<SmartCol> = active_indices
        .iter()
        .map(|&idx| {
            let c = &all_cols[idx];
            SmartCol {
                min_width: c.min_width,
                priority: c.priority,
                constraint: c.default_constraint,
            }
        })
        .collect();

    let (constraints, visible_active_indices) =
        compute_smart_table_layout(&smart_cols, available_width, 1);
    let visible_real_indices: Vec<usize> = visible_active_indices
        .into_iter()
        .filter_map(|idx| active_indices.get(idx).copied())
        .collect();

    (constraints, visible_real_indices)
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

pub fn compute_visible_peer_columns(available_width: u16) -> (Vec<Constraint>, Vec<usize>) {
    let peer_cols = get_peer_columns();
    let smart_peer_cols: Vec<SmartCol> = peer_cols
        .iter()
        .map(|c| SmartCol {
            min_width: c.min_width,
            priority: c.priority,
            constraint: c.default_constraint,
        })
        .collect();

    compute_smart_table_layout(&smart_peer_cols, available_width, 1)
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
