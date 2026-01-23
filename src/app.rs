// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::io::Stdout;

use std::collections::VecDeque;

use magnet_url::Magnet;

use fuzzy_matcher::FuzzyMatcher;

use strum_macros::EnumIter;

use crate::torrent_manager::DiskIoOperation;

use crate::config::{PeerSortColumn, Settings, SortDirection, TorrentSettings, TorrentSortColumn};
use crate::token_bucket::TokenBucket;

use crate::tui::events;
use crate::tui::tree;
use crate::tui::tree::RawNode;
use crate::tui::tree::TreeViewState;
use crate::tui::view::draw;

use crate::config::get_watch_path;
use crate::storage::build_fs_tree;

use crate::resource_manager::ResourceType;

use crate::torrent_file::parser::from_bytes;
use crate::torrent_manager::ManagerCommand;
use crate::torrent_manager::ManagerEvent;
use crate::torrent_manager::TorrentManager;
use crate::torrent_manager::TorrentParameters;

use crate::config::get_app_paths;
use crate::config::save_settings;

use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use tokio::signal;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "dht")]
use mainline::{async_dht::AsyncDht, Dht};
#[cfg(not(feature = "dht"))]
type AsyncDht = ();

use sha1::Digest;
use sha2::Sha256;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use std::time::Duration;

use notify::{Config, Error as NotifyError, Event, RecommendedWatcher, RecursiveMode, Watcher};

use ratatui::prelude::Rect;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::cell::RefCell;
use throbber_widgets_tui::ThrobberState;

use sysinfo::System;

use data_encoding::BASE32;

use tracing::{event as tracing_event, Level};

use crate::resource_manager::{ResourceManager, ResourceManagerClient};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use tokio::time;

use directories::UserDirs;

use ratatui::crossterm::event::{self, Event as CrosstermEvent};

use rand::seq::SliceRandom;
use rand::Rng;

#[cfg(unix)]
use rlimit::Resource;

const SECONDS_HISTORY_MAX: usize = 3600; // 1 hour of per-second data
const MINUTES_HISTORY_MAX: usize = 48 * 60; // 48 hours of per-minute data

const FILE_HANDLE_MINIMUM: usize = 64;
const SAFE_BUDGET_PERCENTAGE: f64 = 0.85;

#[derive(serde::Deserialize)]
struct CratesResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(serde::Deserialize)]
struct CrateInfo {
    max_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilePriority {
    #[default]
    Normal,
    High,
    Skip,
    Mixed, // Used for folders that contain children with different priorities
}

impl FilePriority {
    pub fn next(&self) -> Self {
        match self {
            Self::Normal => Self::Skip,
            Self::Skip => Self::High,
            Self::High => Self::Normal,
            Self::Mixed => Self::Normal, // Reset mixed to Normal on toggle
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TorrentPreviewPayload {
    pub file_index: Option<usize>, // None for folders
    pub size: u64,
    pub priority: FilePriority,
}

// Implement AddAssign so RawNode::from_path_list can aggregate folder sizes
impl std::ops::AddAssign for TorrentPreviewPayload {
    fn add_assign(&mut self, rhs: Self) {
        self.size += rhs.size;
        // Logic to determine folder priority state (e.g., if children differ -> Mixed)
        if self.priority != rhs.priority {
            self.priority = FilePriority::Mixed;
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub enum BrowserPane {
    #[default]
    FileSystem,
    TorrentPreview,
}

#[derive(Default, Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum FileBrowserMode {
    #[default]
    Directory, // User must pick a folder (e.g. Download Location)
    File(Vec<String>), // User must pick a file matching these extensions (e.g. vec!["torrent"])
    // Future proofing: You could add 'AnyFile' or 'FileOrFolder' here later
    DownloadLocSelection {
        torrent_files: Vec<String>, // List of relative file paths in the torrent
        container_name: String,     // Name of the container folder (e.g. hash_name)
        use_container: bool,        // Toggle state
        is_editing_name: bool,      // Whether the user is currently typing the name
        focused_pane: BrowserPane,
        preview_tree: Vec<RawNode<TorrentPreviewPayload>>, // Interactive tree
        preview_state: TreeViewState,                      // Cursor & expansion state for preview
        cursor_pos: usize,
        original_name_backup: String,
    },
    ConfigPathSelection {
        target_item: ConfigItem,
        current_settings: Box<Settings>,
        selected_index: usize,
        items: Vec<ConfigItem>,
    },
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: std::time::SystemTime,
}

#[derive(Debug, Default)]
pub struct ThrobberHolder {
    pub torrent_sparkline: ThrobberState,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum DataRate {
    RateQuarter,
    RateHalf,
    #[default]
    Rate1s,
    Rate2s,
    Rate4s,
    Rate10s,
    Rate20s,
    Rate30s,
    Rate60s,
}

impl DataRate {
    /// Returns the millisecond value for the data rate.
    pub fn as_ms(&self) -> u64 {
        match self {
            DataRate::RateQuarter => 4000,
            DataRate::RateHalf => 2000,
            DataRate::Rate1s => 1000,
            DataRate::Rate2s => 500,
            DataRate::Rate4s => 250,
            DataRate::Rate10s => 100,
            DataRate::Rate20s => 50,
            DataRate::Rate30s => 33,
            DataRate::Rate60s => 17,
        }
    }

    pub fn to_string(self) -> &'static str {
        match self {
            DataRate::RateQuarter => "0.25 FPS",
            DataRate::RateHalf => "0.5 FPS",
            DataRate::Rate1s => "1 FPS",
            DataRate::Rate2s => "2 FPS",
            DataRate::Rate4s => "4 FPS",
            DataRate::Rate10s => "10 FPS",
            DataRate::Rate20s => "20 FPS",
            DataRate::Rate30s => "30 FPS",
            DataRate::Rate60s => "60 FPS",
        }
    }

    /// Cycles to the next (slower) data rate (lower FPS).
    pub fn next_slower(&self) -> Self {
        match self {
            DataRate::Rate60s => DataRate::Rate30s,
            DataRate::Rate30s => DataRate::Rate20s,
            DataRate::Rate20s => DataRate::Rate10s,
            DataRate::Rate10s => DataRate::Rate4s,
            DataRate::Rate4s => DataRate::Rate2s,
            DataRate::Rate2s => DataRate::Rate1s,
            DataRate::Rate1s => DataRate::RateHalf,
            DataRate::RateHalf => DataRate::RateQuarter,
            DataRate::RateQuarter => DataRate::RateQuarter,
        }
    }

    /// Cycles to the previous (faster) data rate (higher FPS).
    pub fn next_faster(&self) -> Self {
        match self {
            DataRate::RateQuarter => DataRate::RateHalf,
            DataRate::RateHalf => DataRate::Rate1s,
            DataRate::Rate1s => DataRate::Rate2s,
            DataRate::Rate2s => DataRate::Rate4s,
            DataRate::Rate4s => DataRate::Rate10s,
            DataRate::Rate10s => DataRate::Rate20s,
            DataRate::Rate20s => DataRate::Rate30s,
            DataRate::Rate30s => DataRate::Rate60s,
            DataRate::Rate60s => DataRate::Rate60s,
        }
    }
}

#[derive(Default, Clone)]
pub struct CalculatedLimits {
    pub reserve_permits: usize,
    pub max_connected_peers: usize,
    pub disk_read_permits: usize,
    pub disk_write_permits: usize,
}
impl CalculatedLimits {
    pub fn into_map(self) -> HashMap<ResourceType, usize> {
        let mut map = HashMap::new();
        map.insert(ResourceType::Reserve, self.reserve_permits);
        map.insert(ResourceType::PeerConnection, self.max_connected_peers);
        map.insert(ResourceType::DiskRead, self.disk_read_permits);
        map.insert(ResourceType::DiskWrite, self.disk_write_permits);
        map
    }
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum GraphDisplayMode {
    OneMinute,
    FiveMinutes,
    #[default]
    TenMinutes,
    ThirtyMinutes,
    OneHour,
    ThreeHours,
    TwelveHours,
    TwentyFourHours,
}

impl GraphDisplayMode {
    pub fn as_seconds(&self) -> usize {
        match self {
            Self::OneMinute => 60,
            Self::FiveMinutes => 300,
            Self::TenMinutes => 600,
            Self::ThirtyMinutes => 1800,
            Self::OneHour => 3600,
            Self::ThreeHours => 3 * 3600,
            Self::TwelveHours => 12 * 3600,
            Self::TwentyFourHours => 86_400,
        }
    }

    pub fn to_string(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinutes => "5m",
            Self::TenMinutes => "10m",
            Self::ThirtyMinutes => "30m",
            Self::OneHour => "1h",
            Self::ThreeHours => "3h",
            Self::TwelveHours => "12h",
            Self::TwentyFourHours => "24h",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::OneMinute => Self::FiveMinutes,
            Self::FiveMinutes => Self::TenMinutes,
            Self::TenMinutes => Self::ThirtyMinutes,
            Self::ThirtyMinutes => Self::OneHour,
            Self::OneHour => Self::ThreeHours,
            Self::ThreeHours => Self::TwelveHours,
            Self::TwelveHours => Self::TwentyFourHours,
            Self::TwentyFourHours => Self::OneMinute,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::OneMinute => Self::TwentyFourHours,
            Self::FiveMinutes => Self::OneMinute,
            Self::TenMinutes => Self::FiveMinutes,
            Self::ThirtyMinutes => Self::TenMinutes,
            Self::OneHour => Self::ThirtyMinutes,
            Self::ThreeHours => Self::OneHour,
            Self::TwelveHours => Self::ThreeHours,
            Self::TwentyFourHours => Self::TwelveHours,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectedHeader {
    Torrent(usize),
    Peer(usize),
}
impl Default for SelectedHeader {
    fn default() -> Self {
        SelectedHeader::Torrent(0)
    }
}

pub enum AppCommand {
    AddTorrentFromFile(PathBuf),
    AddTorrentFromPathFile(PathBuf),
    AddMagnetFromFile(PathBuf),
    ClientShutdown(PathBuf),
    PortFileChanged(PathBuf),
    FetchFileTree {
        path: PathBuf,
        browser_mode: FileBrowserMode,
        highlight_path: Option<PathBuf>,
    },
    UpdateFileBrowserData {
        data: Vec<tree::RawNode<FileMetadata>>,
        highlight_path: Option<PathBuf>,
    },
    UpdateConfig(Settings),
    UpdateVersionAvailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, EnumIter)]
pub enum ConfigItem {
    ClientPort,
    DefaultDownloadFolder,
    WatchFolder,
    GlobalDownloadLimit,
    GlobalUploadLimit,
}

#[derive(Default)]
#[allow(clippy::large_enum_variant)]
pub enum AppMode {
    Welcome,
    #[default]
    Normal,
    PowerSaving,
    DeleteConfirm {
        info_hash: Vec<u8>,
        with_files: bool,
    },
    Config {
        settings_edit: Box<Settings>,
        selected_index: usize,
        items: Vec<ConfigItem>,
        editing: Option<(ConfigItem, String)>,
    },
    FileBrowser {
        state: TreeViewState,
        data: Vec<RawNode<FileMetadata>>,
        browser_mode: FileBrowserMode,
    },
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TorrentControlState {
    #[default]
    Running,
    Paused,
    Deleting,
}

#[derive(Debug, Clone, Default)]
pub struct PeerInfo {
    pub address: String,
    pub peer_id: Vec<u8>,
    pub am_choking: bool,
    pub peer_choking: bool,
    pub am_interested: bool,
    pub peer_interested: bool,
    pub bitfield: Vec<bool>,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub total_downloaded: u64,
    pub total_uploaded: u64,
    pub last_action: String,
}

#[derive(Debug, Default, Clone)]
pub struct TorrentMetrics {
    pub torrent_control_state: TorrentControlState,
    pub info_hash: Vec<u8>,
    pub torrent_or_magnet: String,
    pub torrent_name: String,
    pub download_path: Option<PathBuf>,
    pub container_name: Option<String>,
    pub file_priorities: HashMap<usize, FilePriority>,
    pub number_of_successfully_connected_peers: usize,
    pub number_of_pieces_total: u32,
    pub number_of_pieces_completed: u32,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub bytes_downloaded_this_tick: u64,
    pub bytes_uploaded_this_tick: u64,
    pub eta: Duration,
    pub peers: Vec<PeerInfo>,
    pub activity_message: String,
    pub next_announce_in: Duration,
    pub total_size: u64,
    pub bytes_written: u64,

    pub blocks_in_history: Vec<u64>,
    pub blocks_out_history: Vec<u64>,
    pub blocks_in_this_tick: u64,
    pub blocks_out_this_tick: u64,
}

#[derive(Default, Debug)]
pub struct TorrentDisplayState {
    pub latest_state: TorrentMetrics,
    pub download_history: Vec<u64>,
    pub upload_history: Vec<u64>,

    pub bytes_read_this_tick: u64,
    pub bytes_written_this_tick: u64,
    pub disk_read_speed_bps: u64,
    pub disk_write_speed_bps: u64,
    pub disk_read_history_log: VecDeque<DiskIoOperation>,
    pub disk_write_history_log: VecDeque<DiskIoOperation>,
    pub disk_read_thrash_score: u64,
    pub disk_write_thrash_score: u64,

    pub smoothed_download_speed_bps: u64,
    pub smoothed_upload_speed_bps: u64,

    pub swarm_availability_history: Vec<Vec<u32>>,

    pub peers_discovered_this_tick: u64,
    pub peers_connected_this_tick: u64,
    pub peers_disconnected_this_tick: u64,
    pub peer_discovery_history: Vec<u64>,
    pub peer_connection_history: Vec<u64>,
    pub peer_disconnect_history: Vec<u64>,
}

#[derive(Default)]
pub struct AppState {
    pub update_available: Option<String>,
    pub should_quit: bool,
    pub shutdown_progress: f64,
    pub system_warning: Option<String>,
    pub system_error: Option<String>,
    pub limits: CalculatedLimits,

    pub screen_area: Rect,
    pub mode: AppMode,
    pub show_help: bool,
    pub externally_accessable_port: bool,
    pub anonymize_torrent_names: bool,

    pub pending_torrent_path: Option<PathBuf>,
    pub pending_torrent_link: String,
    pub torrents: HashMap<Vec<u8>, TorrentDisplayState>,

    pub torrent_list_order: Vec<Vec<u8>>,

    pub total_download_history: Vec<u64>,
    pub total_upload_history: Vec<u64>,
    pub avg_download_history: Vec<u64>,
    pub avg_upload_history: Vec<u64>,
    pub disk_backoff_history_ms: VecDeque<u64>,
    pub minute_disk_backoff_history_ms: VecDeque<u64>,
    pub max_disk_backoff_this_tick_ms: u64,

    pub lifetime_downloaded_from_config: u64,
    pub lifetime_uploaded_from_config: u64,

    pub session_total_downloaded: u64,
    pub session_total_uploaded: u64,

    pub cpu_usage: f32,
    pub ram_usage_percent: f32,
    pub avg_disk_read_bps: u64,
    pub avg_disk_write_bps: u64,

    pub disk_read_history: Vec<u64>,
    pub disk_write_history: Vec<u64>,
    pub app_ram_usage: u64,

    pub run_time: u64,

    pub global_disk_read_history_log: VecDeque<DiskIoOperation>,
    pub global_disk_write_history_log: VecDeque<DiskIoOperation>,
    pub global_disk_read_thrash_score: u64,
    pub global_disk_write_thrash_score: u64,

    pub read_op_start_times: VecDeque<Instant>,
    pub write_op_start_times: VecDeque<Instant>,
    pub read_latency_ema: f64,
    pub write_latency_ema: f64,
    pub avg_disk_read_latency: Duration,
    pub avg_disk_write_latency: Duration,
    pub reads_completed_this_tick: u32,
    pub writes_completed_this_tick: u32,
    pub read_iops: u32,
    pub write_iops: u32,

    pub ui_needs_redraw: bool,
    pub data_rate: DataRate,

    pub selected_header: SelectedHeader,
    pub torrent_sort: (TorrentSortColumn, SortDirection),
    pub peer_sort: (PeerSortColumn, SortDirection),
    pub selected_torrent_index: usize,
    pub selected_peer_index: usize,

    pub is_searching: bool,
    pub search_query: String,

    pub graph_mode: GraphDisplayMode,
    pub minute_avg_dl_history: Vec<u64>,
    pub minute_avg_ul_history: Vec<u64>,

    pub last_tuning_score: u64,
    pub current_tuning_score: u64,
    pub tuning_countdown: u64,
    pub last_tuning_limits: CalculatedLimits,
    pub is_seeding: bool,
    pub baseline_speed_ema: f64,
    pub global_disk_thrash_score: f64,
    pub adaptive_max_scpb: f64,
    pub global_seek_cost_per_byte_history: Vec<f64>,

    pub recently_processed_files: HashMap<PathBuf, Instant>,

    pub throbber_holder: RefCell<ThrobberHolder>,
}

pub struct App {
    pub app_state: AppState,
    pub client_configs: Settings,

    pub listener: tokio::net::TcpListener,

    pub torrent_manager_incoming_peer_txs: HashMap<Vec<u8>, Sender<(TcpStream, Vec<u8>)>>,
    pub torrent_manager_command_txs: HashMap<Vec<u8>, Sender<ManagerCommand>>,
    pub distributed_hash_table: AsyncDht,
    pub resource_manager: ResourceManagerClient,
    pub global_dl_bucket: Arc<TokenBucket>,
    pub global_ul_bucket: Arc<TokenBucket>,

    pub torrent_tx: broadcast::Sender<TorrentMetrics>,
    pub torrent_rx: broadcast::Receiver<TorrentMetrics>,
    pub manager_event_tx: mpsc::Sender<ManagerEvent>,
    pub manager_event_rx: mpsc::Receiver<ManagerEvent>,
    pub app_command_tx: mpsc::Sender<AppCommand>,
    pub app_command_rx: mpsc::Receiver<AppCommand>,
    pub tui_event_tx: mpsc::Sender<CrosstermEvent>,
    pub tui_event_rx: mpsc::Receiver<CrosstermEvent>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub tui_task: Option<tokio::task::JoinHandle<()>>,
    pub notify_rx: mpsc::Receiver<Result<Event, NotifyError>>,
    pub watcher: RecommendedWatcher,
}
impl App {
    pub async fn new(client_configs: Settings) -> Result<Self, Box<dyn std::error::Error>> {
        let listener =
            tokio::net::TcpListener::bind(format!("0.0.0.0:{}", client_configs.client_port))
                .await?;

        let (manager_event_tx, manager_event_rx) = mpsc::channel::<ManagerEvent>(1000);
        let (app_command_tx, app_command_rx) = mpsc::channel::<AppCommand>(10);
        let (tui_event_tx, tui_event_rx) = mpsc::channel::<CrosstermEvent>(100);
        let (torrent_tx, torrent_rx) = broadcast::channel::<TorrentMetrics>(100);
        let (shutdown_tx, _) = broadcast::channel(1);

        let (limits, system_warning) = calculate_adaptive_limits(&client_configs);
        tracing_event!(
            Level::DEBUG,
            "Adaptive limits calculated: max_peers={}, disk_reads={}, disk_writes={}",
            limits.max_connected_peers,
            limits.disk_read_permits,
            limits.disk_write_permits
        );
        let mut rm_limits = HashMap::new();
        rm_limits.insert(ResourceType::Reserve, (limits.reserve_permits, 0));
        rm_limits.insert(
            ResourceType::PeerConnection,
            (limits.max_connected_peers, limits.max_connected_peers * 2),
        );
        rm_limits.insert(
            ResourceType::DiskRead,
            (limits.disk_read_permits, limits.disk_read_permits * 2),
        );
        rm_limits.insert(
            ResourceType::DiskWrite,
            (limits.disk_write_permits, limits.disk_read_permits * 2),
        );
        let (resource_manager, resource_manager_client) =
            ResourceManager::new(rm_limits, shutdown_tx.clone());
        tokio::spawn(resource_manager.run());

        #[cfg(feature = "dht")]
        let bootstrap_nodes: Vec<&str> = client_configs
            .bootstrap_nodes
            .iter()
            .map(AsRef::as_ref)
            .collect();

        #[cfg(feature = "dht")]
        let distributed_hash_table = Dht::builder()
            .bootstrap(&bootstrap_nodes)
            .port(client_configs.client_port)
            .server_mode()
            .build()?
            .as_async();

        #[cfg(not(feature = "dht"))]
        let distributed_hash_table = ();

        let dl_limit = client_configs.global_download_limit_bps as f64;
        let ul_limit = client_configs.global_upload_limit_bps as f64;
        let global_dl_bucket = Arc::new(TokenBucket::new(dl_limit, dl_limit));
        let global_ul_bucket = Arc::new(TokenBucket::new(ul_limit, ul_limit));

        let app_state = AppState {
            system_warning,
            system_error: None,
            limits: limits.clone(),
            ui_needs_redraw: true,
            torrent_sort: (
                client_configs.torrent_sort_column,
                client_configs.torrent_sort_direction,
            ),
            peer_sort: (
                client_configs.peer_sort_column,
                client_configs.peer_sort_direction,
            ),
            lifetime_downloaded_from_config: client_configs.lifetime_downloaded,
            lifetime_uploaded_from_config: client_configs.lifetime_uploaded,
            minute_disk_backoff_history_ms: VecDeque::with_capacity(24 * 60),
            max_disk_backoff_this_tick_ms: 0,
            last_tuning_score: 0,
            current_tuning_score: 0,
            tuning_countdown: 90,
            last_tuning_limits: limits.clone(),
            adaptive_max_scpb: 10.0,
            ..Default::default()
        };

        let (notify_tx, notify_rx) = mpsc::channel::<Result<Event, NotifyError>>(100);
        let watcher = Self::create_watcher(&client_configs, notify_tx)?;

        let mut app = Self {
            app_state,
            client_configs: client_configs.clone(),
            listener,
            torrent_manager_incoming_peer_txs: HashMap::new(),
            torrent_manager_command_txs: HashMap::new(),
            distributed_hash_table,
            resource_manager: resource_manager_client,
            global_dl_bucket,
            global_ul_bucket,
            torrent_tx,
            torrent_rx,
            manager_event_tx,
            manager_event_rx,
            app_command_tx,
            app_command_rx,
            tui_event_tx,
            tui_event_rx,
            shutdown_tx,
            tui_task: None,
            watcher,
            notify_rx,
        };

        let mut torrents_to_load = app.client_configs.torrents.clone();
        torrents_to_load.sort_by_key(|t| !t.validation_status);
        for torrent_config in torrents_to_load {
            if torrent_config.torrent_or_magnet.starts_with("magnet:") {
                app.add_magnet_torrent(
                    torrent_config.name.clone(),
                    torrent_config.torrent_or_magnet.clone(),
                    torrent_config.download_path.clone(),
                    torrent_config.validation_status,
                    torrent_config.torrent_control_state,
                    torrent_config.file_priorities,
                    torrent_config.container_name,
                )
                .await;
            } else {
                app.add_torrent_from_file(
                    PathBuf::from(&torrent_config.torrent_or_magnet),
                    torrent_config.download_path.clone(),
                    torrent_config.validation_status,
                    torrent_config.torrent_control_state,
                    torrent_config.file_priorities.clone(),
                    torrent_config.container_name,
                )
                .await;
            }
        }

        if app.app_state.torrents.is_empty() {
            app.app_state.mode = AppMode::Welcome;
        }

        let is_leeching = app.app_state.torrents.values().any(|t| {
            t.latest_state.number_of_pieces_completed < t.latest_state.number_of_pieces_total
        });
        app.app_state.is_seeding = !is_leeching;

        Ok(app)
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(size) = terminal.size() {
            self.app_state.screen_area = Rect::new(0, 0, size.width, size.height);
        }

        self.process_pending_commands().await;

        self.startup_crossterm_event_listener();

        let mut sys = System::new();

        let mut stats_interval = time::interval(Duration::from_secs(1));
        let mut tuning_interval = time::interval(Duration::from_secs(90));
        let mut version_interval = time::interval(Duration::from_secs(24 * 60 * 60));

        self.save_state_to_disk();

        let mut next_draw_time = Instant::now();
        while !self.app_state.should_quit {
            let current_target_framerate = match self.app_state.mode {
                AppMode::Welcome => Duration::from_millis(16), // Force 60 FPS for animation
                AppMode::PowerSaving => Duration::from_secs(1), // Force 1 FPS for Zen mode
                _ => Duration::from_millis(self.app_state.data_rate.as_ms()), // User-defined FPS
            };

            tokio::select! {
                _ = signal::ctrl_c() => {
                    self.app_state.should_quit = true;
                }
                Ok(Ok((stream, _addr))) = tokio::time::timeout(Duration::from_secs(2), self.listener.accept()) => {
                    self.handle_incoming_peer(stream).await;

                }
                Some(event) = self.manager_event_rx.recv() => {
                    self.handle_manager_event(event);
                    self.app_state.ui_needs_redraw = true;
                }

                result = self.torrent_rx.recv() => {
                    self.update_torrent_state(result);
                    self.app_state.ui_needs_redraw = true;
                }

                Some(command) = self.app_command_rx.recv() => {
                    self.handle_app_command(command).await;
                },

                Some(event) = self.tui_event_rx.recv() => {
                    self.clamp_selected_indices();
                    events::handle_event(event, self).await;
                    next_draw_time = Instant::now();
                }

                Some(result) = self.notify_rx.recv() => {
                    self.handle_file_event(result).await;
                }

                _ = stats_interval.tick() => {
                    self.calculate_stats(&mut sys);
                    self.app_state.ui_needs_redraw = true;
                }

                _ = tuning_interval.tick() => {
                    self.tuning_resource_limits().await;
                }

                _ = time::sleep_until(next_draw_time.into()) => {
                    next_draw_time = Instant::now() + current_target_framerate;

                    let force_animation = matches!(self.app_state.mode, AppMode::Welcome);

                    if self.app_state.ui_needs_redraw || force_animation {
                        terminal.draw(|f| {
                            draw(f, &self.app_state, &self.client_configs);
                        })?;
                        self.app_state.ui_needs_redraw = false;
                    }
                }
                _ = version_interval.tick() => {
                    let current_version = env!("CARGO_PKG_VERSION");
                    let tx = self.app_command_tx.clone();
                    let mut shutdown_rx = self.shutdown_tx.subscribe();

                    tokio::spawn(async move {
                        tokio::select! {
                            latest_result = App::fetch_latest_version() => {
                                if let Ok(latest) = latest_result {
                                    if latest != current_version {
                                        tracing::info!("New version found! Current: {} - Latest: {}", current_version, latest.clone());
                                        let _ = tx.send(AppCommand::UpdateVersionAvailable(latest)).await;
                                    }
                                    else {
                                        tracing::info!("Current version is latest! Current: {} - Latest: {}", current_version, latest);
                                    }
                                }
                            }
                            _ = shutdown_rx.recv() => {
                                tracing::debug!("Version check aborted due to shutdown");
                            }
                        }
                    });
                }
            }
        }

        self.save_state_to_disk();

        self.shutdown_sequence(terminal).await;

        Ok(())
    }

    fn startup_crossterm_event_listener(&mut self) {
        let tui_event_tx_clone = self.tui_event_tx.clone();
        let mut tui_shutdown_rx = self.shutdown_tx.subscribe();

        self.tui_task = Some(tokio::spawn(async move {
            loop {
                if tui_shutdown_rx.try_recv().is_ok() {
                    break;
                }

                // Run blocking poll to completion (do NOT use tokio::select!)
                // This ensures we never abandon a thread that is reading from stdin.
                // Keep the timeout relatively short (250ms) so the app remains responsive to shutdown.
                let event =
                    tokio::task::spawn_blocking(|| -> std::io::Result<Option<CrosstermEvent>> {
                        if event::poll(Duration::from_millis(250))? {
                            return Ok(Some(event::read()?));
                        }
                        Ok(None)
                    })
                    .await;

                match event {
                    Ok(Ok(Some(e))) => {
                        if tui_event_tx_clone.send(e).await.is_err() {
                            break;
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => {
                        tracing::error!("Crossterm event error: {}", e);
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Blocking task join error: {}", e);
                        break;
                    }
                }

                if tui_shutdown_rx.try_recv().is_ok() {
                    break;
                }
            }
        }));
    }

    async fn shutdown_sequence(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
        let _ = self.shutdown_tx.send(());

        if let Some(handle) = self.tui_task.take() {
            tracing::info!("Waiting for TUI event listener to finish...");
            if let Err(e) = handle.await {
                tracing::error!("Error joining TUI task: {}", e);
            }
        }

        let total_managers_to_shut_down = self.torrent_manager_command_txs.len();
        let mut managers_shut_down = 0;

        for manager_tx in self.torrent_manager_command_txs.values() {
            let _ = manager_tx.try_send(ManagerCommand::Shutdown);
        }

        if total_managers_to_shut_down == 0 {
            return;
        }

        let shutdown_timeout = time::sleep(Duration::from_secs(5));
        let mut draw_interval = time::interval(Duration::from_millis(100));
        tokio::pin!(shutdown_timeout);

        tracing_event!(
            Level::INFO,
            "Waiting for {} torrents to shut down...",
            total_managers_to_shut_down
        );

        loop {
            self.app_state.shutdown_progress =
                managers_shut_down as f64 / total_managers_to_shut_down as f64;
            let _ = terminal.draw(|f| {
                draw(f, &self.app_state, &self.client_configs);
            });

            tokio::select! {
                Some(event) = self.manager_event_rx.recv() => {
                    match event {
                        ManagerEvent::DeletionComplete(..) => {
                            managers_shut_down += 1;
                            if managers_shut_down == total_managers_to_shut_down {
                                tracing_event!(Level::INFO, "All torrents shut down gracefully.");
                                break;
                            }
                        }
                        _ => {
                            // CRITICAL: We must aggressively drain other events (Stats, BlockReceived, etc.)
                            // so the managers don't get blocked on a full channel while trying to die.
                        }
                    }
                }

                _ = draw_interval.tick() => {
                }

                _ = &mut shutdown_timeout => {
                    tracing_event!(Level::WARN, "Shutdown timed out. {}/{} managers did not reply. Forcing exit.",
                        total_managers_to_shut_down - managers_shut_down,
                        total_managers_to_shut_down
                    );
                    break;
                }
            }
        }
    }

    async fn handle_incoming_peer(&mut self, mut stream: TcpStream) {
        if !self.app_state.externally_accessable_port {
            self.app_state.externally_accessable_port = true;
        }

        let torrent_manager_incoming_peer_txs_clone =
            self.torrent_manager_incoming_peer_txs.clone();
        let resource_manager_clone = self.resource_manager.clone();
        let mut permit_shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let _session_permit = tokio::select! {
                permit_result = resource_manager_clone.acquire_peer_connection() => {
                    match permit_result {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            tracing_event!(Level::DEBUG, "Failed to acquire permit. Manager shut down?");
                            None
                        }
                    }
                }
                _ = permit_shutdown_rx.recv() => {
                    None
                }
            };
            let mut buffer = vec![0u8; 68];
            if (stream.read_exact(&mut buffer).await).is_ok() {
                let peer_info_hash = &buffer[28..48];

                if let Some(torrent_manager_tx) =
                    torrent_manager_incoming_peer_txs_clone.get(peer_info_hash)
                {
                    let torrent_manager_tx_clone = torrent_manager_tx.clone();
                    let _ = torrent_manager_tx_clone.send((stream, buffer)).await;
                } else {
                    tracing::trace!(
                        "ROUTING FAIL: No manager registered for hash: {}",
                        hex::encode(peer_info_hash)
                    );
                }
            }
        });
    }

    async fn handle_app_command(&mut self, command: AppCommand) {
        match command {
            AppCommand::AddTorrentFromFile(path) => {
                // Determine if the file is coming from a watch folder (User or System)
                let parent_dir = path.parent();
                let is_user_watch = self
                    .client_configs
                    .watch_folder
                    .as_ref()
                    .is_some_and(|p| parent_dir == Some(p));

                let system_watch_info = get_watch_path();
                let is_system_watch = system_watch_info
                    .as_ref()
                    .is_some_and(|(p, _)| parent_dir == Some(p));

                if let Some(download_path) = &self.client_configs.default_download_folder {
                    // --- CASE A: Automatic Adding (Default Path Exists) ---
                    self.add_torrent_from_file(
                        path.to_path_buf(),
                        Some(download_path.to_path_buf()),
                        false,
                        TorrentControlState::Running,
                        HashMap::new(),
                        None,
                    )
                    .await;

                    self.save_state_to_disk();

                    // Cleanup: Move to processed or rename to .added
                    if is_user_watch || is_system_watch {
                        let move_successful = if let Some((_, processed_path)) = system_watch_info {
                            (|| {
                                fs::create_dir_all(&processed_path).ok()?;
                                let file_name = path.file_name()?;
                                let new_path = processed_path.join(file_name);
                                fs::rename(&path, &new_path).ok()?;
                                Some(())
                            })()
                            .is_some()
                        } else {
                            false
                        };

                        if !move_successful {
                            let mut new_path = path.clone();
                            new_path.set_extension("torrent.added");
                            let _ = fs::rename(&path, &new_path);
                        }
                    }
                } else {
                    // --- CASE B: Manual Adding (Prompt for Location) ---
                    if let Ok(buffer) = fs::read(&path) {
                        if let Ok(torrent) = from_bytes(&buffer) {
                            // 1. Rename the file immediately if it's in a watch folder
                            // This prevents the watcher from re-triggering while the UI is open.
                            let final_path = if is_user_watch || is_system_watch {
                                let mut new_path = path.clone();
                                new_path.set_extension("torrent.added");
                                if let Err(e) = fs::rename(&path, &new_path) {
                                    tracing::error!("Failed to rename watched file: {}", e);
                                    path.clone()
                                } else {
                                    new_path
                                }
                            } else {
                                path.clone()
                            };

                            // 2. Parse metadata for the UI Preview
                            let info_hash = if torrent.info.meta_version == Some(2) {
                                let mut hasher = Sha256::new();
                                hasher.update(&torrent.info_dict_bencode);
                                hasher.finalize()[0..20].to_vec()
                            } else {
                                let mut hasher = sha1::Sha1::new();
                                hasher.update(&torrent.info_dict_bencode);
                                hasher.finalize().to_vec()
                            };

                            let info_hash_hex = hex::encode(&info_hash);
                            let default_container_name =
                                format!("{} [{}]", torrent.info.name, info_hash_hex);
                            let file_list = torrent.file_list();
                            let should_enclose = file_list.len() > 1;

                            // 3. Build Preview Tree
                            let preview_payloads: Vec<(Vec<String>, TorrentPreviewPayload)> =
                                file_list
                                    .into_iter()
                                    .enumerate()
                                    .map(|(idx, (parts, size))| {
                                        (
                                            parts,
                                            TorrentPreviewPayload {
                                                file_index: Some(idx),
                                                size,
                                                priority: FilePriority::Normal,
                                            },
                                        )
                                    })
                                    .collect();

                            let preview_tree = RawNode::from_path_list(None, preview_payloads);
                            let mut preview_state = TreeViewState::new();
                            for node in &preview_tree {
                                node.expand_all(&mut preview_state);
                            }

                            // 4. Update state and switch to File Browser
                            self.app_state.pending_torrent_path = Some(final_path);
                            let initial_path = self.get_initial_destination_path();

                            let _ = self.app_command_tx.try_send(AppCommand::FetchFileTree {
                                path: initial_path,
                                browser_mode: FileBrowserMode::DownloadLocSelection {
                                    torrent_files: vec![],
                                    container_name: default_container_name.clone(),
                                    use_container: should_enclose,
                                    is_editing_name: false,
                                    preview_tree,
                                    preview_state,
                                    focused_pane: BrowserPane::FileSystem,
                                    cursor_pos: 0,
                                    original_name_backup: default_container_name,
                                },
                                highlight_path: None,
                            });
                        } else {
                            self.app_state.system_error =
                                Some("Failed to parse torrent file for preview.".to_string());
                        }
                    } else {
                        self.app_state.system_error =
                            Some("Failed to read torrent file.".to_string());
                    }
                }
            }
            AppCommand::AddTorrentFromPathFile(path) => {
                if let Some((_, processed_path)) = get_watch_path() {
                    match fs::read_to_string(&path) {
                        Ok(torrent_file_path_str) => {
                            let torrent_file_path = PathBuf::from(torrent_file_path_str.trim());
                            if let Some(download_path) =
                                self.client_configs.default_download_folder.clone()
                            {
                                self.add_torrent_from_file(
                                    torrent_file_path,
                                    Some(download_path),
                                    false,
                                    TorrentControlState::Running,
                                    HashMap::new(),
                                    None,
                                )
                                .await;
                                self.save_state_to_disk();
                            } else {
                                self.app_state.pending_torrent_path = Some(torrent_file_path);
                                let initial_path = self.get_initial_destination_path();

                                let _ = self.app_command_tx.try_send(AppCommand::FetchFileTree {
                                    path: initial_path,
                                    browser_mode: FileBrowserMode::DownloadLocSelection {
                                        torrent_files: vec![],
                                        container_name: "New Torrent".to_string(),
                                        use_container: true,
                                        is_editing_name: false,
                                        preview_tree: Vec::new(),
                                        preview_state: TreeViewState::default(),
                                        focused_pane: BrowserPane::FileSystem,
                                        cursor_pos: 0,
                                        original_name_backup: "New Torrent".to_string(),
                                    },
                                    highlight_path: None,
                                });
                            }
                        }
                        Err(e) => {
                            tracing_event!(
                                Level::ERROR,
                                "Failed to read torrent path from file {:?}: {}",
                                &path,
                                e
                            );
                        }
                    }

                    if let Some(file_name) = path.file_name() {
                        let new_path = processed_path.join(file_name);
                        if let Err(e) = fs::rename(&path, &new_path) {
                            tracing_event!(
                                Level::WARN,
                                "Failed to move processed path file {:?}: {}",
                                &path,
                                e
                            );
                        }
                    }
                }
            }
            AppCommand::AddMagnetFromFile(path) => {
                if let Some((_, processed_path)) = get_watch_path() {
                    match fs::read_to_string(&path) {
                        Ok(magnet_link) => {
                            if let Some(download_path) =
                                self.client_configs.default_download_folder.clone()
                            {
                                self.add_magnet_torrent(
                                    "Fetching name...".to_string(),
                                    magnet_link.trim().to_string(),
                                    Some(download_path),
                                    false,
                                    TorrentControlState::Running,
                                    HashMap::new(),
                                    None,
                                )
                                .await;
                                self.save_state_to_disk();
                            } else {
                                self.app_state.pending_torrent_link = magnet_link;
                                let initial_path = self.get_initial_destination_path();

                                let _ = self.app_command_tx.try_send(AppCommand::FetchFileTree {
                                    path: initial_path,
                                    browser_mode: FileBrowserMode::DownloadLocSelection {
                                        torrent_files: vec![],
                                        container_name: "Magnet Download".to_string(), // Default name for magnets
                                        use_container: true,
                                        is_editing_name: false,
                                        preview_tree: Vec::new(), // Magnets start with empty metadata
                                        preview_state: TreeViewState::default(),
                                        focused_pane: BrowserPane::FileSystem,
                                        cursor_pos: 0,
                                        original_name_backup: "Magnet Download".to_string(),
                                    },
                                    highlight_path: None,
                                });
                            }
                        }
                        Err(e) => {
                            tracing_event!(
                                Level::ERROR,
                                "Failed to read magnet file {:?}: {}",
                                &path,
                                e
                            );
                        }
                    }

                    if let Err(e) = fs::create_dir_all(&processed_path) {
                        tracing_event!(
                            Level::ERROR,
                            "Could not create processed files directory: {}",
                            e
                        );
                    } else if let Some(file_name) = path.file_name() {
                        let new_path = processed_path.join(file_name);
                        if let Err(e) = fs::rename(&path, &new_path) {
                            tracing_event!(
                                Level::ERROR,
                                "Failed to move processed magnet file {:?}: {}",
                                &path,
                                e
                            );
                        }
                    }
                } else {
                    tracing_event!(
                        Level::ERROR,
                        "Could not get system watch paths for magnet processing."
                    );
                }
            }
            AppCommand::ClientShutdown(path) => {
                tracing_event!(Level::INFO, "Shutdown command received via command file.");
                self.app_state.should_quit = true;
                if let Err(e) = fs::remove_file(&path) {
                    tracing_event!(
                        Level::WARN,
                        "Failed to remove command file {:?}: {}",
                        &path,
                        e
                    );
                }
            }
            AppCommand::PortFileChanged(path) => {
                self.handle_port_change(path).await;
            }

            AppCommand::FetchFileTree {
                path,
                browser_mode,
                highlight_path,
            } => {
                let tx = self.app_command_tx.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                let path_clone = path.clone();
                let highlight_clone = highlight_path.clone();

                // 1. Update or Initialize the UI state immediately
                if let AppMode::FileBrowser { state, .. } = &mut self.app_state.mode {
                    // If already in browser, just update the path we are viewing
                    state.current_path = path.clone();
                } else {
                    // Otherwise, initialize the mode
                    let mut tree_state = crate::tui::tree::TreeViewState::new();
                    tree_state.current_path = path.clone();
                    self.app_state.mode = AppMode::FileBrowser {
                        state: tree_state,
                        data: Vec::new(),
                        browser_mode,
                    };
                }

                // 2. Spawn the background crawl
                tokio::spawn(async move {
                    tokio::select! {
                        result = build_fs_tree(&path_clone, 0) => {
                            if let Ok(nodes) = result {
                                // Pass the highlight_path back so the Update arm can find it
                                let _ = tx.send(AppCommand::UpdateFileBrowserData {
                                    data: nodes,
                                    highlight_path: highlight_clone
                                }).await;
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            tracing::debug!("Aborting FileBrowser crawl due to shutdown");
                        }
                    }
                });
            }

            AppCommand::UpdateFileBrowserData {
                mut data,
                highlight_path,
            } => {
                if let AppMode::FileBrowser {
                    state,
                    data: existing_data,
                    browser_mode,
                } = &mut self.app_state.mode
                {
                    // --- 1. Apply Dynamic Sorting ---
                    if let FileBrowserMode::File(extensions) = browser_mode {
                        let target_exts: Vec<String> =
                            extensions.iter().map(|e| e.to_lowercase()).collect();
                        data.sort_by(|a, b| {
                            let a_matches = target_exts
                                .iter()
                                .any(|ext| a.name.to_lowercase().ends_with(ext));
                            let b_matches = target_exts
                                .iter()
                                .any(|ext| b.name.to_lowercase().ends_with(ext));

                            // 1. Priority: Torrents first
                            if a_matches != b_matches {
                                return b_matches.cmp(&a_matches);
                            }

                            // 2. Priority: Folders second (ensures folders follow torrents directly)
                            if a.is_dir != b.is_dir {
                                return b.is_dir.cmp(&a.is_dir); // Changed order to put folders higher
                            }

                            // 3. Final: Sort by newest date
                            b.payload.modified.cmp(&a.payload.modified)
                        });
                    }

                    // --- 2. Update Data ---
                    *existing_data = data;
                    state.top_most_offset = 0;

                    // --- 3. Smart Cursor Positioning ---
                    if let Some(target) = highlight_path {
                        // Find the index of the folder/file we want to highlight
                        if let Some(index) = existing_data
                            .iter()
                            .position(|node| node.full_path == target)
                        {
                            state.cursor_path = Some(target);

                            // Adjust scroll if the item is below the current visible area
                            let area = crate::tui::formatters::centered_rect(
                                75,
                                80,
                                self.app_state.screen_area,
                            );
                            let max_height = area.height.saturating_sub(2) as usize;
                            if index >= max_height {
                                state.top_most_offset = index.saturating_sub(max_height / 2);
                            }
                        } else {
                            state.cursor_path =
                                existing_data.first().map(|node| node.full_path.clone());
                        }
                    } else {
                        // Default: reset to top if entering a new folder
                        state.cursor_path =
                            existing_data.first().map(|node| node.full_path.clone());
                    }

                    self.app_state.ui_needs_redraw = true;
                }
            }
            AppCommand::UpdateConfig(new_settings) => {
                let old_settings = self.client_configs.clone();
                self.client_configs = new_settings.clone();

                // 1. Handle Port Change (Re-bind Listener)
                if new_settings.client_port != old_settings.client_port {
                    tracing::info!(
                        "Config update: Port changed to {}",
                        new_settings.client_port
                    );
                    // Reuse your existing port logic or extract it to a helper
                    self.rebind_listener(new_settings.client_port).await;
                }

                // 2. Handle Bandwidth Limit Changes (Update Buckets)
                if new_settings.global_download_limit_bps != old_settings.global_download_limit_bps
                {
                    self.global_dl_bucket
                        .set_rate(new_settings.global_download_limit_bps as f64);
                }
                if new_settings.global_upload_limit_bps != old_settings.global_upload_limit_bps {
                    self.global_ul_bucket
                        .set_rate(new_settings.global_upload_limit_bps as f64);
                }

                if new_settings.watch_folder != old_settings.watch_folder {
                    if let Some(old_path) = old_settings.watch_folder {
                        if let Err(e) = self.watcher.unwatch(&old_path) {
                            tracing::info!("Failed to unwatch old folder {:?}: {}", old_path, e);
                        }
                    }

                    if let Some(new_path) = &self.client_configs.watch_folder {
                        if let Err(e) = self.watcher.watch(new_path, RecursiveMode::NonRecursive) {
                            tracing::error!("Failed to watch new folder: {}", e);
                        }
                    }
                }

                // 3. Persist to Disk
                self.save_state_to_disk();

                // 4. Force Redraw
                self.app_state.ui_needs_redraw = true;
            }

            AppCommand::UpdateVersionAvailable(latest_version) => {
                self.app_state.update_available = Some(latest_version);
            }
        }
    }

    fn handle_manager_event(&mut self, event: ManagerEvent) {
        match event {
            ManagerEvent::DeletionComplete(info_hash, result) => {
                if let Err(e) = result {
                    tracing_event!(Level::ERROR, "Deletion failed for torrent: {}", e);
                }

                self.client_configs.torrents.retain(|t| {
                    let t_info_hash = if t.torrent_or_magnet.starts_with("magnet:") {
                        Magnet::new(&t.torrent_or_magnet)
                            .ok()
                            .and_then(|m| m.hash().map(|s| s.to_string()))
                            .and_then(|hash_str| decode_info_hash(&hash_str).ok())
                    } else {
                        PathBuf::from(&t.torrent_or_magnet)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .and_then(|s| hex::decode(s).ok())
                    };

                    match t_info_hash {
                        Some(t_hash) => t_hash != info_hash,
                        None => true,
                    }
                });

                self.app_state.torrents.remove(&info_hash);
                self.torrent_manager_command_txs.remove(&info_hash);
                self.torrent_manager_incoming_peer_txs.remove(&info_hash);
                self.app_state
                    .torrent_list_order
                    .retain(|ih| *ih != info_hash);

                if self.app_state.selected_torrent_index >= self.app_state.torrent_list_order.len()
                    && !self.app_state.torrent_list_order.is_empty()
                {
                    self.app_state.selected_torrent_index =
                        self.app_state.torrent_list_order.len() - 1;
                }

                self.save_state_to_disk();

                self.app_state.ui_needs_redraw = true;
            }
            ManagerEvent::DiskReadStarted { info_hash, op } => {
                self.app_state
                    .read_op_start_times
                    .push_front(Instant::now());
                self.app_state.global_disk_read_history_log.push_front(op);
                self.app_state.global_disk_read_history_log.truncate(100);
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.bytes_read_this_tick += op.length as u64;
                    torrent.disk_read_history_log.push_front(op);
                    torrent.disk_read_history_log.truncate(50);
                }
            }
            ManagerEvent::DiskReadFinished => {
                if let Some(start_time) = self.app_state.read_op_start_times.pop_front() {
                    let duration = start_time.elapsed();
                    const LATENCY_EMA_PERIOD: f64 = 10.0;
                    let alpha = 2.0 / (LATENCY_EMA_PERIOD + 1.0);
                    let current_micros = duration.as_micros() as f64;

                    let new_ema = if self.app_state.read_latency_ema == 0.0 {
                        current_micros
                    } else {
                        (current_micros * alpha) + (self.app_state.read_latency_ema * (1.0 - alpha))
                    };

                    self.app_state.read_latency_ema = new_ema;
                    self.app_state.avg_disk_read_latency = Duration::from_micros(new_ema as u64);
                }
                self.app_state.reads_completed_this_tick += 1;
            }
            ManagerEvent::DiskWriteStarted { info_hash, op } => {
                self.app_state
                    .write_op_start_times
                    .push_front(Instant::now());
                self.app_state.global_disk_write_history_log.push_front(op);
                self.app_state.global_disk_write_history_log.truncate(100);
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.bytes_written_this_tick += op.length as u64;
                    torrent.disk_write_history_log.push_front(op);
                    torrent.disk_write_history_log.truncate(50);
                }
            }
            ManagerEvent::DiskWriteFinished => {
                if let Some(start_time) = self.app_state.write_op_start_times.pop_front() {
                    let duration = start_time.elapsed();
                    const LATENCY_EMA_PERIOD: f64 = 10.0;
                    let alpha = 2.0 / (LATENCY_EMA_PERIOD + 1.0);
                    let current_micros = duration.as_micros() as f64;

                    let new_ema = if self.app_state.write_latency_ema == 0.0 {
                        current_micros
                    } else {
                        (current_micros * alpha)
                            + (self.app_state.write_latency_ema * (1.0 - alpha))
                    };

                    self.app_state.write_latency_ema = new_ema;
                    self.app_state.avg_disk_write_latency = Duration::from_micros(new_ema as u64);
                }
                self.app_state.writes_completed_this_tick += 1;
            }
            ManagerEvent::DiskIoBackoff { duration } => {
                let duration_ms = duration.as_millis() as u64;
                self.app_state.max_disk_backoff_this_tick_ms = self
                    .app_state
                    .max_disk_backoff_this_tick_ms
                    .max(duration_ms);

                if self.app_state.system_warning.is_none() {
                    let warning_msg = "System Warning: Potential FD limit hit (detected via Disk I/O backoff). Increase 'ulimit -n' if issues persist.".to_string();
                    self.app_state.system_warning = Some(warning_msg);
                }
            }
            ManagerEvent::PeerDiscovered { info_hash } => {
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.peers_discovered_this_tick += 1;
                }
            }
            ManagerEvent::PeerConnected { info_hash } => {
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.peers_connected_this_tick += 1;
                }
            }
            ManagerEvent::PeerDisconnected { info_hash } => {
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.peers_disconnected_this_tick += 1;
                }
            }
            ManagerEvent::BlockReceived { info_hash } => {
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.latest_state.blocks_in_this_tick += 1;
                }
            }
            ManagerEvent::BlockSent { info_hash } => {
                if let Some(torrent) = self.app_state.torrents.get_mut(&info_hash) {
                    torrent.latest_state.blocks_out_this_tick += 1;
                }
            }

            ManagerEvent::MetadataLoaded { info_hash, torrent } => {
                if let AppMode::FileBrowser {
                    browser_mode:
                        FileBrowserMode::DownloadLocSelection {
                            preview_tree,
                            preview_state,
                            container_name,
                            original_name_backup,
                            use_container,
                            ..
                        },
                    ..
                } = &mut self.app_state.mode
                {
                    // 1. REDUNDANCY GUARD: Check if metadata was already processed
                    // If the tree is already populated, ignore subsequent peer metadata arrivals
                    if !preview_tree.is_empty() {
                        tracing::debug!(target: "superseedr", "Metadata already hydrated for {:?}, ignoring redundant peer update", hex::encode(&info_hash));
                        return;
                    }

                    // 2. Build the tree payloads
                    let file_list = torrent.file_list();
                    let payloads: Vec<(Vec<String>, TorrentPreviewPayload)> = file_list
                        .into_iter()
                        .enumerate()
                        .map(|(idx, (parts, size))| {
                            (
                                parts,
                                TorrentPreviewPayload {
                                    file_index: Some(idx),
                                    size,
                                    priority: FilePriority::Normal,
                                },
                            )
                        })
                        .collect();

                    // 3. Hydrate the tree structure
                    let has_multiple_files = payloads.len() > 1;
                    *preview_tree = RawNode::from_path_list(None, payloads);

                    // 4. Update Display Name and State
                    let info_hash_hex = hex::encode(&info_hash);
                    let name = format!("{} [{}]", torrent.info.name, &info_hash_hex);
                    *container_name = name.clone();
                    *original_name_backup = name;
                    *use_container = has_multiple_files;

                    // 5. INITIALIZE UI STATE: Set the initial cursor
                    if let Some(first) = preview_tree.first() {
                        preview_state.cursor_path = Some(std::path::PathBuf::from(&first.name));
                    }

                    // 6. Auto-expand all folders
                    for node in preview_tree.iter_mut() {
                        node.expand_all(preview_state);
                    }

                    // 7. Force UI redraw
                    self.app_state.ui_needs_redraw = true;
                    tracing::info!(target: "superseedr", "Magnet preview tree hydrated (first arrival)");
                }


                if let Some(state) = self.app_state.torrents.get_mut(&info_hash) {
                    if state.latest_state.download_path.is_some() 
                        && state.latest_state.container_name.is_none() 
                        && torrent.file_list().len() > 1 
                    {
                        let info_hash_hex = hex::encode(&info_hash);
                        let name_suffix = format!("{} [{}]", torrent.info.name, info_hash_hex);
                        
                        // 1. Update UI so user sees the folder name
                        state.latest_state.container_name = Some(name_suffix.clone());

                        // 2. Update Manager so storage uses the folder
                        if let Some(tx) = self.torrent_manager_command_txs.get(&info_hash) {
                            let _ = tx.try_send(ManagerCommand::SetUserTorrentConfig {
                                torrent_data_path: state.latest_state.download_path.clone().unwrap(),
                                file_priorities: state.latest_state.file_priorities.clone(),
                                container_name: Some(name_suffix), 
                            });
                        }
                    }
                }
            }
        }
    }

    async fn handle_file_event(&mut self, result: Result<Event, notify::Error>) {
        match result {
            Ok(event) => {
                if event.kind.is_create() || event.kind.is_modify() {
                    const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

                    for path in &event.paths {
                        if path.to_string_lossy().ends_with(".tmp") {
                            tracing_event!(Level::DEBUG, "Skipping temporary file: {:?}", path);
                            continue;
                        }

                        let now = Instant::now();
                        if let Some(last_time) = self.app_state.recently_processed_files.get(path) {
                            if now.duration_since(*last_time) < DEBOUNCE_DURATION {
                                tracing_event!(
                                    Level::DEBUG,
                                    "Skipping file {:?} due to debounce.",
                                    path
                                );
                                continue;
                            }
                        }

                        self.app_state
                            .recently_processed_files
                            .insert(path.clone(), now);
                        tracing_event!(
                            Level::DEBUG,
                            "Processing file event: {:?} for path: {:?}",
                            event.kind,
                            path
                        );

                        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                            match ext {
                                "torrent" => {
                                    let _ = self
                                        .app_command_tx
                                        .send(AppCommand::AddTorrentFromFile(path.clone()))
                                        .await;
                                }
                                "path" => {
                                    let _ = self
                                        .app_command_tx
                                        .send(AppCommand::AddTorrentFromPathFile(path.clone()))
                                        .await;
                                }
                                "magnet" => {
                                    let _ = self
                                        .app_command_tx
                                        .send(AppCommand::AddMagnetFromFile(path.clone()))
                                        .await;
                                }
                                _ => {}
                            }
                        }

                        if path.file_name().is_some_and(|name| name == "shutdown.cmd") {
                            tracing_event!(Level::INFO, "Shutdown command detected: {:?}", path);
                            let _ = self
                                .app_command_tx
                                .send(AppCommand::ClientShutdown(path.clone()))
                                .await;
                        }

                        if path
                            .file_name()
                            .is_some_and(|name| name == "forwarded_port")
                        {
                            tracing_event!(Level::INFO, "Port file change detected: {:?}", path);
                            let _ = self
                                .app_command_tx
                                .send(AppCommand::PortFileChanged(path.clone()))
                                .await;
                        }
                    }
                }
            }
            Err(error) => {
                tracing_event!(Level::ERROR, "File watcher error: {:?}", error);
            }
        }
    }

    async fn handle_port_change(&mut self, path: PathBuf) {
        tracing_event!(Level::DEBUG, "Processing port file change...");
        let port_str = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing_event!(Level::ERROR, "Failed to read port file {:?}: {}", &path, e);
                return;
            }
        };

        match port_str.trim().parse::<u16>() {
            Ok(new_port) => {
                if new_port > 0 && new_port != self.client_configs.client_port {
                    tracing_event!(
                        Level::INFO,
                        "Port changed: {} -> {}. Attempting to re-bind listener.",
                        self.client_configs.client_port,
                        new_port
                    );

                    match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", new_port)).await {
                        Ok(new_listener) => {
                            self.listener = new_listener;
                            self.client_configs.client_port = new_port;

                            tracing_event!(
                                Level::INFO,
                                "Successfully bound to new port {}",
                                new_port
                            );

                            // Persist the new port immediately
                            self.save_state_to_disk();

                            // Notify all running managers
                            for manager_tx in self.torrent_manager_command_txs.values() {
                                let _ =
                                    manager_tx.try_send(ManagerCommand::UpdateListenPort(new_port));
                            }

                            // Rebuild DHT if enabled
                            #[cfg(feature = "dht")]
                            {
                                tracing::event!(Level::INFO, "Rebinding DHT server to new port...");
                                let bootstrap_nodes: Vec<&str> = self
                                    .client_configs
                                    .bootstrap_nodes
                                    .iter()
                                    .map(AsRef::as_ref)
                                    .collect();

                                match Dht::builder()
                                    .bootstrap(&bootstrap_nodes)
                                    .port(new_port)
                                    .server_mode()
                                    .build()
                                {
                                    Ok(new_dht_server) => {
                                        let new_dht_handle = new_dht_server.as_async();
                                        self.distributed_hash_table = new_dht_handle.clone();

                                        for manager_tx in self.torrent_manager_command_txs.values()
                                        {
                                            let _ = manager_tx.try_send(
                                                ManagerCommand::UpdateDhtHandle(
                                                    new_dht_handle.clone(),
                                                ),
                                            );
                                        }
                                        tracing::event!(
                                            Level::INFO,
                                            "DHT server rebound and handles updated."
                                        );
                                    }
                                    Err(e) => {
                                        tracing::event!(
                                            Level::ERROR,
                                            "Failed to build new DHT server: {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing_event!(
                                Level::ERROR,
                                "Failed to bind to new port {}: {}. Retaining old listener.",
                                new_port,
                                e
                            );
                        }
                    }
                } else if new_port == self.client_configs.client_port {
                    tracing_event!(
                        Level::DEBUG,
                        "Port file updated, but port is unchanged ({}).",
                        new_port
                    );
                }
            }
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to parse new port from file {:?}: {}",
                    &path,
                    e
                );
            }
        }
    }

    fn calculate_stats(&mut self, sys: &mut System) {
        self.app_state
            .throbber_holder
            .borrow_mut()
            .torrent_sparkline
            .calc_next();

        if matches!(self.app_state.mode, AppMode::PowerSaving)
            && !self.app_state.run_time.is_multiple_of(5)
        {
            self.app_state.run_time += 1;
            return;
        }

        let pid = match sysinfo::get_current_pid() {
            Ok(pid) => pid,
            Err(e) => {
                tracing_event!(Level::ERROR, "Could not get current PID: {}", e);
                return;
            }
        };

        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);

        if let Some(process) = sys.process(pid) {
            self.app_state.cpu_usage = process.cpu_usage() / sys.cpus().len() as f32;
            self.app_state.app_ram_usage = process.memory();
            self.app_state.ram_usage_percent =
                (process.memory() as f32 / sys.total_memory() as f32) * 100.0;
            self.app_state.run_time = process.run_time();
        }

        // --- Calculate all thrash scores ---
        self.app_state.global_disk_read_thrash_score =
            calculate_thrash_score(&self.app_state.global_disk_read_history_log);
        self.app_state.global_disk_write_thrash_score =
            calculate_thrash_score(&self.app_state.global_disk_write_history_log);

        let global_read_thrash_f64 =
            calculate_thrash_score_seek_cost_f64(&self.app_state.global_disk_read_history_log);
        let global_write_thrash_f64 =
            calculate_thrash_score_seek_cost_f64(&self.app_state.global_disk_write_history_log);
        self.app_state.global_disk_thrash_score = global_read_thrash_f64 + global_write_thrash_f64;

        if self.app_state.global_disk_thrash_score > 0.01 {
            self.app_state
                .global_seek_cost_per_byte_history
                .push(self.app_state.global_disk_thrash_score);
        }
        if self.app_state.global_seek_cost_per_byte_history.len() > 1000 {
            self.app_state.global_seek_cost_per_byte_history.remove(0);
        }
        const MIN_SAMPLES_TO_LEARN: usize = 50;
        if self.app_state.global_seek_cost_per_byte_history.len() > MIN_SAMPLES_TO_LEARN {
            let mut sorted_history = self.app_state.global_seek_cost_per_byte_history.clone();
            sorted_history.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let percentile_index = (sorted_history.len() as f64 * 0.95) as usize;
            let new_scpb_max = sorted_history[percentile_index];
            self.app_state.adaptive_max_scpb = new_scpb_max.max(1.0);
        }

        let mut global_disk_read_bps = 0;
        let mut global_disk_write_bps = 0;

        for torrent in self.app_state.torrents.values_mut() {
            torrent.disk_read_speed_bps = torrent.bytes_read_this_tick * 8;
            torrent.disk_write_speed_bps = torrent.bytes_written_this_tick * 8;

            global_disk_read_bps += torrent.disk_read_speed_bps;
            global_disk_write_bps += torrent.disk_write_speed_bps;

            torrent.bytes_read_this_tick = 0;
            torrent.bytes_written_this_tick = 0;

            torrent.disk_read_thrash_score = calculate_thrash_score(&torrent.disk_read_history_log);
            torrent.disk_write_thrash_score =
                calculate_thrash_score(&torrent.disk_write_history_log);

            torrent
                .peer_discovery_history
                .push(torrent.peers_discovered_this_tick);
            torrent
                .peer_connection_history
                .push(torrent.peers_connected_this_tick);
            torrent
                .peer_disconnect_history
                .push(torrent.peers_disconnected_this_tick);
            torrent.peers_discovered_this_tick = 0;
            torrent.peers_connected_this_tick = 0;
            torrent.peers_disconnected_this_tick = 0;
            if torrent.peer_discovery_history.len() > 200 {
                torrent.peer_discovery_history.remove(0);
                torrent.peer_connection_history.remove(0);
                torrent.peer_disconnect_history.remove(0);
            }

            torrent
                .latest_state
                .blocks_in_history
                .push(torrent.latest_state.blocks_in_this_tick);
            torrent
                .latest_state
                .blocks_out_history
                .push(torrent.latest_state.blocks_out_this_tick);
            torrent.latest_state.blocks_in_this_tick = 0;
            torrent.latest_state.blocks_out_this_tick = 0;
            if torrent.latest_state.blocks_in_history.len() > 200 {
                torrent.latest_state.blocks_in_history.remove(0);
                torrent.latest_state.blocks_out_history.remove(0);
            }
        }

        // Update the global history with the new, accurate totals
        self.app_state.disk_read_history.push(global_disk_read_bps);
        self.app_state
            .disk_write_history
            .push(global_disk_write_bps);
        if self.app_state.disk_read_history.len() > 60 {
            self.app_state.disk_read_history.remove(0);
            self.app_state.disk_write_history.remove(0);
        }

        self.app_state.avg_disk_read_bps = if self.app_state.disk_read_history.is_empty() {
            0
        } else {
            self.app_state.disk_read_history.iter().sum::<u64>()
                / self.app_state.disk_read_history.len() as u64
        };
        self.app_state.avg_disk_write_bps = if self.app_state.disk_write_history.is_empty() {
            0
        } else {
            self.app_state.disk_write_history.iter().sum::<u64>()
                / self.app_state.disk_write_history.len() as u64
        };

        let mut total_dl = 0;
        let mut total_ul = 0;
        for torrent in self.app_state.torrents.values() {
            total_dl += torrent.smoothed_download_speed_bps;
            total_ul += torrent.smoothed_upload_speed_bps;
        }

        self.app_state.total_download_history.push(total_dl);
        self.app_state.total_upload_history.push(total_ul);
        self.app_state.avg_download_history.push(total_dl);
        self.app_state.avg_upload_history.push(total_ul);

        self.app_state.read_iops = self.app_state.reads_completed_this_tick;
        self.app_state.write_iops = self.app_state.writes_completed_this_tick;
        self.app_state.reads_completed_this_tick = 0;
        self.app_state.writes_completed_this_tick = 0;

        // Record the maximum backoff duration seen during the tick that just ended
        self.app_state
            .disk_backoff_history_ms
            .push_back(self.app_state.max_disk_backoff_this_tick_ms);
        if self.app_state.disk_backoff_history_ms.len() > SECONDS_HISTORY_MAX {
            self.app_state.disk_backoff_history_ms.pop_front();
        }

        // System Runtime calculations ==================================
        let run_time = self.app_state.run_time;
        if run_time > 0 && run_time.is_multiple_of(60) {
            let history_len = self.app_state.disk_backoff_history_ms.len();
            let start_index = history_len.saturating_sub(60);

            let backoff_slice_ms =
                &self.app_state.disk_backoff_history_ms.make_contiguous()[start_index..];
            let max_backoff_in_minute_ms = backoff_slice_ms.iter().max().copied().unwrap_or(0);
            self.app_state
                .minute_disk_backoff_history_ms
                .push_back(max_backoff_in_minute_ms);
            if self.app_state.minute_disk_backoff_history_ms.len() > MINUTES_HISTORY_MAX {
                self.app_state.minute_disk_backoff_history_ms.pop_front();
            }

            let seconds_dl = &self.app_state.avg_download_history;
            let minute_slice_dl = &seconds_dl[seconds_dl.len().saturating_sub(60)..];
            if !minute_slice_dl.is_empty() {
                let minute_avg_dl =
                    minute_slice_dl.iter().sum::<u64>() / minute_slice_dl.len() as u64;
                self.app_state.minute_avg_dl_history.push(minute_avg_dl);
            }

            let seconds_ul = &self.app_state.avg_upload_history;
            let minute_slice_ul = &seconds_ul[seconds_ul.len().saturating_sub(60)..];
            if !minute_slice_ul.is_empty() {
                let minute_avg_ul =
                    minute_slice_ul.iter().sum::<u64>() / minute_slice_ul.len() as u64;
                self.app_state.minute_avg_ul_history.push(minute_avg_ul);
            }
        }
        self.app_state.max_disk_backoff_this_tick_ms = 0;

        if self.app_state.avg_download_history.len() > SECONDS_HISTORY_MAX {
            self.app_state.avg_download_history.remove(0);
            self.app_state.avg_upload_history.remove(0);
        }
        if self.app_state.minute_avg_dl_history.len() > MINUTES_HISTORY_MAX {
            self.app_state.minute_avg_dl_history.remove(0);
            self.app_state.minute_avg_ul_history.remove(0);
        }

        // Check if the primary objective (seeding vs. leeching) has changed.
        let is_leeching = self.app_state.torrents.values().any(|t| {
            t.latest_state.number_of_pieces_completed < t.latest_state.number_of_pieces_total
        });
        let is_seeding = !is_leeching;

        // If the objective has changed, reset the tuning baseline immediately.
        if is_seeding != self.app_state.is_seeding {
            tracing_event!(
                Level::DEBUG,
                "Self-Tune: Objective changed to {}. Resetting score.",
                if is_seeding { "Seeding" } else { "Leeching" }
            );
            self.app_state.last_tuning_score = 0;
            self.app_state.current_tuning_score = 0;
            self.app_state.last_tuning_limits = self.app_state.limits.clone();

            if is_seeding {
                self.app_state.torrent_sort = (TorrentSortColumn::Up, SortDirection::Descending);
                self.app_state.peer_sort = (PeerSortColumn::UL, SortDirection::Descending);
            } else {
                self.app_state.torrent_sort = (TorrentSortColumn::Down, SortDirection::Descending);
                self.app_state.peer_sort = (PeerSortColumn::DL, SortDirection::Descending);
            }
        }
        self.app_state.is_seeding = is_seeding;
        self.app_state.tuning_countdown = self.app_state.tuning_countdown.saturating_sub(1);
    }

    fn update_torrent_state(
        &mut self,
        result: Result<TorrentMetrics, broadcast::error::RecvError>,
    ) {
        match result {
            Ok(message) => {
                self.app_state.session_total_downloaded += message.bytes_downloaded_this_tick;
                self.app_state.session_total_uploaded += message.bytes_uploaded_this_tick;

                let display_state = self
                    .app_state
                    .torrents
                    .entry(message.info_hash)
                    .or_default();

                display_state
                    .latest_state
                    .number_of_successfully_connected_peers =
                    message.number_of_successfully_connected_peers;
                display_state.latest_state.number_of_pieces_total = message.number_of_pieces_total;
                display_state.latest_state.number_of_pieces_completed =
                    message.number_of_pieces_completed;
                display_state.latest_state.download_speed_bps = message.download_speed_bps;
                display_state.latest_state.upload_speed_bps = message.upload_speed_bps;
                display_state.latest_state.eta = message.eta;
                display_state.latest_state.next_announce_in = message.next_announce_in;

                if let Some(path) = message.download_path {
                    display_state.latest_state.download_path = Some(path);
                }
                if !message.torrent_name.is_empty() {
                    display_state.latest_state.torrent_name = message.torrent_name;
                }
                display_state.latest_state.total_size = message.total_size;
                display_state.latest_state.bytes_written = message.bytes_written;

                display_state
                    .download_history
                    .push(display_state.latest_state.download_speed_bps);
                display_state
                    .upload_history
                    .push(display_state.latest_state.upload_speed_bps);

                if display_state.download_history.len() > 200 {
                    display_state.download_history.remove(0);
                    display_state.upload_history.remove(0);
                }

                if self.app_state.total_download_history.len() > 200 {
                    self.app_state.total_download_history.remove(0);
                    self.app_state.total_upload_history.remove(0);
                }

                display_state.smoothed_download_speed_bps =
                    display_state.latest_state.download_speed_bps;
                display_state.smoothed_upload_speed_bps =
                    display_state.latest_state.upload_speed_bps;
                display_state.latest_state.peers = message.peers;

                display_state.latest_state.activity_message = message.activity_message;

                let current_swarm_availability = aggregate_peers_to_availability(
                    &display_state.latest_state.peers,
                    display_state.latest_state.number_of_pieces_total as usize,
                );
                if !display_state.latest_state.peers.is_empty()
                    && !current_swarm_availability.is_empty()
                {
                    display_state
                        .swarm_availability_history
                        .push(current_swarm_availability);
                }
                if display_state.swarm_availability_history.len() > 200 {
                    display_state.swarm_availability_history.remove(0);
                }

                self.sort_and_filter_torrent_list();
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing_event!(Level::DEBUG, "TUI metrics lagged, skipped {} updates", n);
            }
            Err(broadcast::error::RecvError::Closed) => {}
        }
    }

    async fn tuning_resource_limits(&mut self) {
        self.app_state.tuning_countdown = 90;
        let history = if !self.app_state.is_seeding {
            &self.app_state.avg_download_history
        } else {
            &self.app_state.avg_upload_history
        };

        let relevant_history = &history[history.len().saturating_sub(60)..];
        let new_raw_score = if relevant_history.is_empty() {
            0
        } else {
            relevant_history.iter().sum::<u64>() / relevant_history.len() as u64
        };
        let current_scpb = self.app_state.global_disk_thrash_score;
        let scpb_max = self.app_state.adaptive_max_scpb;
        let penalty_factor = (current_scpb / scpb_max - 1.0).max(0.0);
        let new_score = (new_raw_score as f64 / (1.0 + penalty_factor)) as u64;
        self.app_state.current_tuning_score = new_score;

        const BASELINE_ALPHA: f64 = 0.1; // Slower-moving average
        let new_score_f64 = new_score as f64;
        if self.app_state.baseline_speed_ema == 0.0 {
            self.app_state.baseline_speed_ema = new_score_f64;
        } else {
            self.app_state.baseline_speed_ema = (new_score_f64 * BASELINE_ALPHA)
                + (self.app_state.baseline_speed_ema * (1.0 - BASELINE_ALPHA));
        }

        let best_score = self.app_state.last_tuning_score;
        if new_score > best_score {
            self.app_state.last_tuning_score = new_score;
            self.app_state.last_tuning_limits = self.app_state.limits.clone();
            tracing_event!(
                Level::DEBUG,
                "Self-Tune: SUCCESS. New best score: {} (raw: {}, penalty: {:.2}x)",
                new_score,
                new_raw_score,
                penalty_factor
            );
        } else {
            self.app_state.limits = self.app_state.last_tuning_limits.clone();

            let baseline_u64 = self.app_state.baseline_speed_ema as u64;

            const REALITY_CHECK_FACTOR: f64 = 2.0;
            if best_score > 10_000
                && best_score > (self.app_state.baseline_speed_ema * REALITY_CHECK_FACTOR) as u64
            {
                self.app_state.last_tuning_score = baseline_u64;
                tracing_event!(Level::DEBUG, "Self-Tune: REALITY CHECK. Score {} (raw: {}) failed. Old best {} is stale vs. baseline {}. Resetting best to baseline.", new_score, new_raw_score, best_score, baseline_u64);
            } else {
                tracing_event!(Level::DEBUG, "Self-Tune: REVERTING. Score {} (raw: {}, penalty: {:.2}x) was not better than {}. (Baseline is {})", new_score, new_raw_score, penalty_factor, best_score, baseline_u64);
            }

            let _ = self
                .resource_manager
                .update_limits(self.app_state.limits.clone().into_map())
                .await;
        }

        let (next_limits, desc) = make_random_adjustment(self.app_state.limits.clone());
        self.app_state.limits = next_limits;

        tracing_event!(Level::DEBUG, "Self-Tune: Trying next change... {}", desc);
        let _ = self
            .resource_manager
            .update_limits(self.app_state.limits.clone().into_map())
            .await;
    }

    fn save_state_to_disk(&mut self) {
        self.client_configs.lifetime_downloaded = self.app_state.lifetime_downloaded_from_config
            + self.app_state.session_total_downloaded;
        self.client_configs.lifetime_uploaded =
            self.app_state.lifetime_uploaded_from_config + self.app_state.session_total_uploaded;

        self.client_configs.torrent_sort_column = self.app_state.torrent_sort.0;
        self.client_configs.torrent_sort_direction = self.app_state.torrent_sort.1;
        self.client_configs.peer_sort_column = self.app_state.peer_sort.0;
        self.client_configs.peer_sort_direction = self.app_state.peer_sort.1;

        let old_validation_statuses: HashMap<String, bool> = self
            .client_configs
            .torrents
            .iter()
            .map(|cfg| (cfg.torrent_or_magnet.clone(), cfg.validation_status))
            .collect();

        self.client_configs.torrents = self
            .app_state
            .torrents
            .values()
            .map(|torrent| {
                let torrent_state = &torrent.latest_state;

                let is_complete = torrent_state.number_of_pieces_total > 0
                    && torrent_state.number_of_pieces_total
                        == torrent_state.number_of_pieces_completed;

                let old_status = old_validation_statuses
                    .get(&torrent_state.torrent_or_magnet)
                    .cloned()
                    .unwrap_or(false);

                let final_validation_status = if is_complete { true } else { old_status };

                TorrentSettings {
                    torrent_or_magnet: torrent_state.torrent_or_magnet.clone(),
                    name: torrent_state.torrent_name.clone(),
                    validation_status: final_validation_status,
                    download_path: torrent_state.download_path.clone(),
                    container_name: torrent_state.container_name.clone(),
                    torrent_control_state: torrent_state.torrent_control_state.clone(),
                    file_priorities: torrent_state.file_priorities.clone(),
                }
            })
            .collect();

        if let Err(e) = save_settings(&self.client_configs) {
            tracing_event!(Level::ERROR, "Failed to auto-save settings: {}", e);
        } else {
            tracing_event!(Level::DEBUG, "Settings auto-saved successfully.");
        }
    }

    // Constantly ensures all table selected indices are in-bounds
    fn clamp_selected_indices(&mut self) {
        let torrent_count = self.app_state.torrent_list_order.len();

        if torrent_count == 0 {
            self.app_state.selected_torrent_index = 0;
        } else if self.app_state.selected_torrent_index >= torrent_count {
            self.app_state.selected_torrent_index = torrent_count - 1;
        }

        let peer_count = self
            .app_state
            .torrent_list_order
            .get(self.app_state.selected_torrent_index)
            .and_then(|info_hash| self.app_state.torrents.get(info_hash))
            .map_or(0, |torrent| torrent.latest_state.peers.len());

        if peer_count == 0 {
            self.app_state.selected_peer_index = 0;
        } else if self.app_state.selected_peer_index >= peer_count {
            self.app_state.selected_peer_index = peer_count - 1;
        }
    }

    pub fn sort_and_filter_torrent_list(&mut self) {
        let torrents_map = &self.app_state.torrents;
        let (sort_by, sort_direction) = self.app_state.torrent_sort;
        let search_query = &self.app_state.search_query;

        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();

        let mut torrent_list: Vec<Vec<u8>> = torrents_map.keys().cloned().collect();

        if !search_query.is_empty() {
            torrent_list.retain(|info_hash| {
                let torrent_name = torrents_map
                    .get(info_hash)
                    .map_or("", |t| &t.latest_state.torrent_name);

                matcher.fuzzy_match(torrent_name, search_query).is_some()
            });
        }

        torrent_list.sort_by(|a_info_hash, b_info_hash| {
            let Some(a_torrent) = torrents_map.get(a_info_hash) else {
                return std::cmp::Ordering::Equal;
            };
            let Some(b_torrent) = torrents_map.get(b_info_hash) else {
                return std::cmp::Ordering::Equal;
            };

            let ordering = match sort_by {
                TorrentSortColumn::Name => a_torrent
                    .latest_state
                    .torrent_name
                    .cmp(&b_torrent.latest_state.torrent_name),
                TorrentSortColumn::Down => b_torrent
                    .smoothed_download_speed_bps
                    .cmp(&a_torrent.smoothed_download_speed_bps),
                TorrentSortColumn::Up => b_torrent
                    .smoothed_upload_speed_bps
                    .cmp(&a_torrent.smoothed_upload_speed_bps),
                TorrentSortColumn::Progress => {
                    let calc_progress = |t: &TorrentDisplayState| -> f64 {
                        if t.latest_state.number_of_pieces_total == 0 {
                            0.0
                        } else {
                            t.latest_state.number_of_pieces_completed as f64
                                / t.latest_state.number_of_pieces_total as f64
                        }
                    };

                    let a_prog = calc_progress(a_torrent);
                    let b_prog = calc_progress(b_torrent);

                    // Standard float comparison
                    a_prog.total_cmp(&b_prog)
                }
            };

            let default_direction = match sort_by {
                TorrentSortColumn::Name => SortDirection::Ascending,
                _ => SortDirection::Descending,
            };

            let primary_ordering = if sort_direction != default_direction {
                ordering.reverse()
            } else {
                ordering
            };

            // If primary sort is equal (e.g. both 0 DL), use activity score.
            primary_ordering.then_with(|| {
                let calculate_weighted_activity = |t: &TorrentDisplayState| -> u64 {
                    // Still look at 60s window to break ties if last 5s are quiet
                    let window = 60;
                    let mut score = 0;

                    let mut sum_vec = |history: &Vec<u64>| {
                        // iter().rev() means index 0 is the most recent second
                        for (i, &count) in history.iter().rev().take(window).enumerate() {
                            if count > 0 {
                                // WEIGHTING LOGIC:
                                // If within the last 5 seconds (indices 0-4), apply heavy weight.
                                // Otherwise, apply a nominal weight of 1.
                                let weight = if i < 5 {
                                    // Example: 0s ago = 50, 1s ago = 40, ... 4s ago = 10
                                    (5 - i) as u64 * 10
                                } else {
                                    1
                                };
                                score += count * weight;
                            }
                        }
                    };

                    sum_vec(&t.peer_discovery_history);
                    sum_vec(&t.peer_connection_history);
                    sum_vec(&t.peer_disconnect_history);

                    score
                };

                let a_activity = calculate_weighted_activity(a_torrent);
                let b_activity = calculate_weighted_activity(b_torrent);

                // Sort Descending
                b_activity.cmp(&a_activity)
            })
        });

        self.app_state.torrent_list_order = torrent_list;
        self.clamp_selected_indices();
    }

    pub fn find_most_common_download_path(&mut self) -> Option<PathBuf> {
        let mut counts: HashMap<PathBuf, usize> = HashMap::new();

        for state in self.app_state.torrents.values() {
            if let Some(download_path) = &state.latest_state.download_path {
                if let Some(parent_path) = download_path.parent() {
                    *counts.entry(parent_path.to_path_buf()).or_insert(0) += 1;
                }
            }
        }

        counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(path, _)| path)
    }

    pub fn get_initial_source_path(&self) -> PathBuf {
        UserDirs::new()
            .and_then(|ud| ud.download_dir().map(|p| p.to_path_buf()))
            .or_else(|| UserDirs::new().map(|ud| ud.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub fn get_initial_destination_path(&mut self) -> PathBuf {
        self.find_most_common_download_path()
            .or_else(|| UserDirs::new().and_then(|ud| ud.download_dir().map(|p| p.to_path_buf())))
            .or_else(|| UserDirs::new().map(|ud| ud.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub async fn add_torrent_from_file(
        &mut self,
        path: PathBuf,
        download_path: Option<PathBuf>,
        is_validated: bool,
        torrent_control_state: TorrentControlState,
        file_priorities: HashMap<usize, FilePriority>,
        container_name: Option<String>,
    ) {
        let buffer = match fs::read(&path) {
            Ok(buf) => buf,
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to read torrent file {:?}: {}",
                    &path,
                    e
                );
                return;
            }
        };

        let torrent = match from_bytes(&buffer) {
            Ok(t) => t,
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to parse torrent file {:?}: {}",
                    &path,
                    e
                );
                return;
            }
        };

        #[cfg(all(feature = "dht", feature = "pex"))]
        {
            if torrent.info.private == Some(1) {
                tracing_event!(
                    Level::ERROR,
                    "Rejected private torrent '{}' in normal build.",
                    torrent.info.name
                );
                self.app_state.system_error = Some(format!(
                    "Private Torrent Rejected:'{}' This build (with DHT/PEX) is not safe for private trackers. Please use private builds for this torrent.",
                    torrent.info.name
                ));
                return;
            }
        }

        let info_hash = if torrent.info.meta_version == Some(2) {
            if !torrent.info.pieces.is_empty() {
                let mut hasher = sha1::Sha1::new();
                hasher.update(&torrent.info_dict_bencode);
                hasher.finalize().to_vec()
            } else {
                // Pure V2 -> Primary is V2 (SHA-256 Truncated)
                let mut hasher = Sha256::new();
                hasher.update(&torrent.info_dict_bencode);
                hasher.finalize()[0..20].to_vec()
            }
        } else {
            // V1 -> SHA-1
            let mut hasher = sha1::Sha1::new();
            hasher.update(&torrent.info_dict_bencode);
            hasher.finalize().to_vec()
        };

        if self.app_state.torrents.contains_key(&info_hash) {
            tracing_event!(
                Level::INFO,
                "Ignoring already present torrent: {}",
                torrent.info.name
            );
            return;
        }

        let torrent_files_dir = match get_app_paths() {
            Some((_, data_dir)) => data_dir.join("torrents"),
            None => {
                tracing_event!(
                    Level::ERROR,
                    "Could not determine application data directory."
                );
                return;
            }
        };
        if let Err(e) = fs::create_dir_all(&torrent_files_dir) {
            tracing_event!(
                Level::ERROR,
                "Could not create torrents data directory: {}",
                e
            );
            return;
        }
        let permanent_torrent_path =
            torrent_files_dir.join(format!("{}.torrent", hex::encode(&info_hash)));
        if let Err(e) = fs::copy(&path, &permanent_torrent_path) {
            tracing_event!(
                Level::ERROR,
                "Failed to copy torrent to data directory: {}",
                e
            );
            return;
        }

        let number_of_pieces_total = if !torrent.info.pieces.is_empty() {
            (torrent.info.pieces.len() / 20) as u32
        } else {
            // Handle v2 torrents (empty pieces list)
            let total_len = torrent.info.total_length();
            if torrent.info.piece_length > 0 {
                // ceil(total_len / piece_length)
                ((total_len as f64) / (torrent.info.piece_length as f64)).ceil() as u32
            } else {
                0
            }
        };

        let placeholder_state = TorrentDisplayState {
            latest_state: TorrentMetrics {
                torrent_control_state: torrent_control_state.clone(),
                info_hash: info_hash.clone(),
                torrent_or_magnet: permanent_torrent_path.to_string_lossy().to_string(),
                torrent_name: torrent.info.name.clone(),
                download_path: download_path.clone(),
                container_name: container_name.clone(),
                number_of_pieces_total,
                file_priorities: file_priorities.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        self.app_state
            .torrents
            .insert(info_hash.clone(), placeholder_state);
        self.app_state.torrent_list_order.push(info_hash.clone());

        let (incoming_peer_tx, incoming_peer_rx) = mpsc::channel::<(TcpStream, Vec<u8>)>(100);
        self.torrent_manager_incoming_peer_txs
            .insert(info_hash.clone(), incoming_peer_tx);
        let (manager_command_tx, manager_command_rx) = mpsc::channel::<ManagerCommand>(100);
        self.torrent_manager_command_txs
            .insert(info_hash.clone(), manager_command_tx);

        let torrent_tx_clone = self.torrent_tx.clone();
        let manager_event_tx_clone = self.manager_event_tx.clone();
        let resource_manager_clone = self.resource_manager.clone();
        let global_dl_bucket_clone = self.global_dl_bucket.clone();
        let global_ul_bucket_clone = self.global_ul_bucket.clone();

        #[cfg(feature = "dht")]
        let dht_clone = self.distributed_hash_table.clone();
        #[cfg(not(feature = "dht"))]
        let dht_clone = ();

        let file_list = torrent.file_list();
        let effective_container_name = if container_name.is_none() && file_list.len() > 1 {
            let info_hash_hex = hex::encode(&info_hash);
            Some(format!("{} [{}]", torrent.info.name, info_hash_hex))
        } else {
            container_name
        };

        let torrent_params = TorrentParameters {
            dht_handle: dht_clone,
            incoming_peer_rx,
            metrics_tx: torrent_tx_clone,
            torrent_validation_status: is_validated,
            torrent_data_path: download_path,
            container_name: effective_container_name.clone(),
            manager_command_rx,
            manager_event_tx: manager_event_tx_clone,
            settings: Arc::clone(&Arc::new(self.client_configs.clone())),
            resource_manager: resource_manager_clone,
            global_dl_bucket: global_dl_bucket_clone,
            global_ul_bucket: global_ul_bucket_clone,
            file_priorities: file_priorities.clone(),
        };

        match TorrentManager::from_torrent(torrent_params, torrent) {
            Ok(torrent_manager) => {
                tokio::spawn(async move {
                    let _ = torrent_manager
                        .run(torrent_control_state == TorrentControlState::Paused)
                        .await;
                });
            }
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to create torrent manager from file: {:?}",
                    e
                );
                self.app_state.torrents.remove(&info_hash);
                self.app_state
                    .torrent_list_order
                    .retain(|ih| *ih != info_hash);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_magnet_torrent(
        &mut self,
        torrent_name: String,
        magnet_link: String,
        download_path: Option<PathBuf>,
        is_validated: bool,
        torrent_control_state: TorrentControlState,
        file_priorities: HashMap<usize, FilePriority>,
        container_name: Option<String>,
    ) {
        tracing::info!(target: "magnet_flow", "Engine: add_magnet_torrent entry. Link: {}", magnet_link); //
        let magnet = match Magnet::new(&magnet_link) {
            Ok(m) => m,
            Err(e) => {
                tracing_event!(Level::ERROR, "Could not parse invalid magnet: {:?}", e);
                return;
            }
        };

        let (v1_hash, v2_hash) = parse_hybrid_hashes(&magnet_link);
        let info_hash = v1_hash
            .clone()
            .or_else(|| v2_hash.clone())
            .expect("Magnet link missing both btih and btmh hashes");

        if self.app_state.torrents.contains_key(&info_hash) {
            if let Some(path) = download_path {
                if let Some(manager_tx) = self.torrent_manager_command_txs.get(&info_hash) {
                    let _ = manager_tx.try_send(ManagerCommand::SetUserTorrentConfig {
                        torrent_data_path: path,
                        file_priorities: file_priorities.clone(),
                        container_name,
                    });
                }
            }
            tracing_event!(Level::INFO, "Updated path for existing torrent from magnet");
            return;
        }

        let placeholder_state = TorrentDisplayState {
            latest_state: TorrentMetrics {
                torrent_control_state: torrent_control_state.clone(),
                info_hash: info_hash.clone(),
                torrent_or_magnet: magnet_link.clone(),
                torrent_name,
                download_path: download_path.clone(),
                container_name: container_name.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        self.app_state
            .torrents
            .insert(info_hash.clone(), placeholder_state);
        self.app_state.torrent_list_order.push(info_hash.clone());

        let (incoming_peer_tx, incoming_peer_rx) = mpsc::channel::<(TcpStream, Vec<u8>)>(100);
        self.torrent_manager_incoming_peer_txs
            .insert(info_hash.clone(), incoming_peer_tx);
        let (manager_command_tx, manager_command_rx) = mpsc::channel::<ManagerCommand>(100);
        self.torrent_manager_command_txs
            .insert(info_hash.clone(), manager_command_tx);

        let dht_clone = self.distributed_hash_table.clone();
        let torrent_tx_clone = self.torrent_tx.clone();
        let manager_event_tx_clone = self.manager_event_tx.clone();
        let resource_manager_clone = self.resource_manager.clone();
        let global_dl_bucket_clone = self.global_dl_bucket.clone();
        let global_ul_bucket_clone = self.global_ul_bucket.clone();
        let torrent_params = TorrentParameters {
            dht_handle: dht_clone,
            incoming_peer_rx,
            metrics_tx: torrent_tx_clone,
            torrent_validation_status: is_validated,
            torrent_data_path: download_path.clone(),
            container_name: container_name.clone(),
            manager_command_rx,
            manager_event_tx: manager_event_tx_clone,
            settings: Arc::clone(&Arc::new(self.client_configs.clone())),
            resource_manager: resource_manager_clone,
            global_dl_bucket: global_dl_bucket_clone,
            global_ul_bucket: global_ul_bucket_clone,
            file_priorities: file_priorities.clone(),
        };

        match TorrentManager::from_magnet(torrent_params, magnet, &magnet_link) {
            Ok(torrent_manager) => {
                tokio::spawn(async move {
                    let _ = torrent_manager
                        .run(torrent_control_state == TorrentControlState::Paused)
                        .await;
                });
            }
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to create new torrent manager from magnet: {:?}",
                    e
                );
                self.app_state.torrents.remove(&info_hash);
                self.app_state
                    .torrent_list_order
                    .retain(|ih| *ih != info_hash);
            }
        }
    }

    async fn process_pending_commands(&mut self) {
        if let Some(user_watch_path) = &self.client_configs.watch_folder {
            if let Ok(entries) = fs::read_dir(user_watch_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    if path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|s| s.starts_with('.'))
                    {
                        continue;
                    }

                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        let command = match ext {
                            "torrent" => Some(AppCommand::AddTorrentFromFile(path.clone())),
                            "path" => Some(AppCommand::AddTorrentFromPathFile(path.clone())),
                            "magnet" => Some(AppCommand::AddMagnetFromFile(path.clone())),
                            _ => None,
                        };

                        if let Some(cmd) = command {
                            let _ = self.app_command_tx.send(cmd).await;
                        }
                    }
                }
            } else {
                tracing_event!(
                    Level::WARN,
                    "Failed to read user watch directory: {:?}",
                    user_watch_path
                );
            }
        }

        if let Some((watch_path, _)) = get_watch_path() {
            let Ok(entries) = fs::read_dir(watch_path) else {
                return;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let command = match ext {
                        "torrent" => Some(AppCommand::AddTorrentFromFile(path.clone())),
                        "path" => Some(AppCommand::AddTorrentFromPathFile(path.clone())),
                        "magnet" => Some(AppCommand::AddMagnetFromFile(path.clone())),
                        "cmd" if path.file_name().is_some_and(|name| name == "shutdown.cmd") => {
                            Some(AppCommand::ClientShutdown(path.clone()))
                        }
                        _ => None,
                    };

                    if let Some(cmd) = command {
                        let _ = self.app_command_tx.send(cmd).await;
                    }
                }
            }
        }
    }

    async fn rebind_listener(&mut self, new_port: u16) {
        match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", new_port)).await {
            Ok(new_listener) => {
                self.listener = new_listener;
                // Note: client_configs.client_port is likely already updated by the caller (UpdateConfig)
                // but we ensure consistency here just in case.
                self.client_configs.client_port = new_port;

                tracing_event!(
                    Level::INFO,
                    "Successfully rebound listener to port {}",
                    new_port
                );

                // Notify all running managers of the new port
                for manager_tx in self.torrent_manager_command_txs.values() {
                    let _ = manager_tx.try_send(ManagerCommand::UpdateListenPort(new_port));
                }

                // Re-initialize DHT if enabled (Logic copied from handle_port_change)
                #[cfg(feature = "dht")]
                {
                    let bootstrap_nodes: Vec<&str> = self
                        .client_configs
                        .bootstrap_nodes
                        .iter()
                        .map(AsRef::as_ref)
                        .collect();

                    match Dht::builder()
                        .bootstrap(&bootstrap_nodes)
                        .port(new_port)
                        .server_mode()
                        .build()
                    {
                        Ok(new_dht_server) => {
                            let new_dht_handle = new_dht_server.as_async();
                            self.distributed_hash_table = new_dht_handle.clone();

                            for manager_tx in self.torrent_manager_command_txs.values() {
                                let _ = manager_tx.try_send(ManagerCommand::UpdateDhtHandle(
                                    new_dht_handle.clone(),
                                ));
                            }
                        }
                        Err(e) => {
                            tracing_event!(
                                Level::ERROR,
                                "Failed to rebuild DHT on new port: {}",
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to bind to new port {}: {}. Listener not updated.",
                    new_port,
                    e
                );
            }
        }
    }

    async fn fetch_latest_version() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::builder()
            .user_agent("superseedr (https://github.com/Jagalite/superseedr)")
            .build()?;

        let url = "https://crates.io/api/v1/crates/superseedr";
        let resp: CratesResponse = client.get(url).send().await?.json().await?;

        Ok(resp.krate.max_version)
    }

    fn create_watcher(
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

        // Watch User Configured Folder
        if let Some(path) = &settings.watch_folder {
            if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
                tracing_event!(Level::ERROR, "Failed to watch user path {:?}: {}", path, e);
            } else {
                tracing_event!(Level::INFO, "Watching user path: {:?}", path);
            }
        }

        // Watch System Folders
        if let Some((watch_path, _)) = get_watch_path() {
            if let Err(e) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
                tracing_event!(
                    Level::ERROR,
                    "Failed to watch system path {:?}: {}",
                    watch_path,
                    e
                );
            }
        }

        // Watch Port File Directory
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
}

fn calculate_thrash_score(history_log: &VecDeque<DiskIoOperation>) -> u64 {
    if history_log.len() < 2 {
        return 0;
    }

    let mut total_seek_distance = 0;
    let mut last_offset_end: Option<u64> = None;

    for op in history_log.iter().rev() {
        if let Some(prev_offset_end) = last_offset_end {
            total_seek_distance += op.offset.abs_diff(prev_offset_end);
        }
        last_offset_end = Some(op.offset + op.length as u64);
    }

    let seek_count = history_log.len() - 1;
    total_seek_distance / seek_count as u64
}

fn calculate_thrash_score_seek_cost_f64(history_log: &VecDeque<DiskIoOperation>) -> f64 {
    if history_log.len() < 2 {
        return 0.0;
    }

    let mut total_seek_distance = 0;
    let mut total_bytes_transferred = 0;
    let mut last_offset_end: Option<u64> = None;

    for op in history_log.iter().rev() {
        if let Some(prev_offset_end) = last_offset_end {
            total_seek_distance += op.offset.abs_diff(prev_offset_end);
        }
        last_offset_end = Some(op.offset + op.length as u64);
        total_bytes_transferred += op.length as u64;
    }

    if total_bytes_transferred == 0 {
        return 0.0;
    }

    total_seek_distance as f64 / total_bytes_transferred as f64
}

fn calculate_adaptive_limits(client_configs: &Settings) -> (CalculatedLimits, Option<String>) {
    let effective_limit;
    let mut system_warning = None;
    const RECOMMENDED_MINIMUM: usize = 1024;

    if let Some(override_val) = client_configs.resource_limit_override {
        effective_limit = override_val;
        if effective_limit < RECOMMENDED_MINIMUM {
            system_warning = Some(format!(
                "Warning: Resource limit is set to {}. Performance may be degraded. Consider increasing with 'ulimit -n 65536'.",
                effective_limit
            ));
        }
    } else {
        #[cfg(unix)]
        {
            if let Ok((soft_limit, _)) = Resource::NOFILE.get() {
                effective_limit = soft_limit as usize;
                if effective_limit < RECOMMENDED_MINIMUM {
                    system_warning = Some(format!(
                        "Warning: System file handle limit is {}. Consider increasing with 'ulimit -n 65536'.",
                        effective_limit
                    ));
                }
            } else {
                effective_limit = RECOMMENDED_MINIMUM;
            }
        }
        #[cfg(windows)]
        {
            effective_limit = 8192;
        }
        #[cfg(not(any(unix, windows)))]
        {
            effective_limit = RECOMMENDED_MINIMUM;
        }
    }

    if let Some(warning) = &system_warning {
        tracing_event!(Level::WARN, "{}", warning);
    }

    let available_budget_after_reservation = effective_limit.saturating_sub(FILE_HANDLE_MINIMUM);
    let safe_budget = available_budget_after_reservation as f64 * SAFE_BUDGET_PERCENTAGE;
    const PEER_PROPORTION: f64 = 0.70;
    const DISK_READ_PROPORTION: f64 = 0.15;
    const DISK_WRITE_PROPORTION: f64 = 0.15;

    let limits = CalculatedLimits {
        reserve_permits: 0,
        max_connected_peers: (safe_budget * PEER_PROPORTION).max(10.0) as usize,
        disk_read_permits: (safe_budget * DISK_READ_PROPORTION).max(4.0) as usize,
        disk_write_permits: (safe_budget * DISK_WRITE_PROPORTION).max(4.0) as usize,
    };

    (limits, system_warning)
}

const MIN_STEP_RATE: f64 = 0.01;
const MAX_STEP_RATE: f64 = 0.10;

// --- Define Min/Max bounds for all resource types ---
const MIN_PEERS: usize = 20;
const MIN_DISK: usize = 2;
const MIN_RESERVE: usize = 0;

// --- Maximum attempts to find a valid trade per cycle ---
const MAX_TRADE_ATTEMPTS: usize = 5;

fn get_limit(limits: &CalculatedLimits, resource: ResourceType) -> usize {
    match resource {
        ResourceType::PeerConnection => limits.max_connected_peers,
        ResourceType::DiskRead => limits.disk_read_permits,
        ResourceType::DiskWrite => limits.disk_write_permits,
        ResourceType::Reserve => limits.reserve_permits,
    }
}

fn set_limit(limits: &mut CalculatedLimits, resource: ResourceType, value: usize) {
    match resource {
        ResourceType::PeerConnection => limits.max_connected_peers = value,
        ResourceType::DiskRead => limits.disk_read_permits = value,
        ResourceType::DiskWrite => limits.disk_write_permits = value,
        ResourceType::Reserve => limits.reserve_permits = value,
    }
}

/// Makes a random, proportional trade, retrying a few times if the first is blocked.
/// This version is refactored to support any number of resources, including Reserve.
fn make_random_adjustment(mut limits: CalculatedLimits) -> (CalculatedLimits, String) {
    let mut rng = rand::rng();
    let mut parameters = [
        ResourceType::PeerConnection,
        ResourceType::DiskRead,
        ResourceType::DiskWrite,
        ResourceType::Reserve, // Add Reserve to the trading pool
    ];

    for attempt in 0..MAX_TRADE_ATTEMPTS {
        parameters.shuffle(&mut rng);
        let source_param = parameters[0];
        let dest_param = parameters[1];

        let source_val = get_limit(&limits, source_param);
        let dest_val = get_limit(&limits, dest_param);

        let source_min = match source_param {
            ResourceType::PeerConnection => MIN_PEERS,
            ResourceType::DiskRead => MIN_DISK,
            ResourceType::DiskWrite => MIN_DISK,
            ResourceType::Reserve => MIN_RESERVE,
        };

        let step_rate = rng.random_range(MIN_STEP_RATE..=MAX_STEP_RATE);
        let amount_to_trade = ((source_val as f64 * step_rate).ceil() as usize).max(1);

        let can_give = source_val >= source_min.saturating_add(amount_to_trade);

        if can_give {
            // --- VALID TRADE FOUND ---

            set_limit(
                &mut limits,
                source_param,
                source_val.saturating_sub(amount_to_trade),
            );
            set_limit(
                &mut limits,
                dest_param,
                dest_val.saturating_add(amount_to_trade),
            );

            let description = format!(
                "Traded {} from {:?} to {:?} (Attempt {})",
                amount_to_trade,
                source_param,
                dest_param,
                attempt + 1
            );
            // Return immediately with the successful trade
            return (limits, description);
        }
        // If trade wasn't possible, the loop continues to the next attempt...
    }

    // --- NO VALID TRADE FOUND after all attempts ---
    // Return the original limits unchanged
    let description = format!(
        "Skipped all trade attempts ({}) this cycle: blocked by bounds",
        MAX_TRADE_ATTEMPTS
    );
    (limits, description)
}

pub fn decode_info_hash(hash_string: &str) -> Result<Vec<u8>, String> {
    // Try Hex Decoding (Handles standard V1 and Hex-encoded V2 Multihash)
    if let Ok(bytes) = hex::decode(hash_string) {
        // V1: 20 bytes (SHA-1)
        if bytes.len() == 20 {
            return Ok(bytes);
        }
        // V2: 34 bytes (Multihash: 2 byte prefix + 32 byte SHA-256)
        // Prefix 0x12 (SHA2-256) + 0x20 (32 bytes)
        if bytes.len() == 34 && bytes[0] == 0x12 && bytes[1] == 0x20 {
            // Return truncated 20 bytes for internal ID
            return Ok(bytes[2..22].to_vec());
        }
    }

    // Try Base32 Decoding (Handles Base32-encoded V1 and V2)
    if let Ok(bytes) = BASE32.decode(hash_string.to_uppercase().as_bytes()) {
        if bytes.len() == 20 {
            return Ok(bytes);
        }
        if bytes.len() == 34 && bytes[0] == 0x12 && bytes[1] == 0x20 {
            return Ok(bytes[2..22].to_vec());
        }
    }

    Err(format!("Invalid info_hash format/length: {}", hash_string))
}

fn aggregate_peers_to_availability(peers: &[PeerInfo], total_pieces: usize) -> Vec<u32> {
    if total_pieces == 0 {
        return Vec::new();
    }
    let mut availability: Vec<u32> = vec![0; total_pieces];
    for peer in peers {
        for (i, has_piece) in peer.bitfield.iter().enumerate().take(total_pieces) {
            if *has_piece {
                availability[i] += 1;
            }
        }
    }
    availability
}

pub fn parse_hybrid_hashes(magnet_link: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let v1 = magnet_link
        .split('&')
        .find(|part| part.contains("xt=urn:btih:"))
        .and_then(|part| part.split(':').next_back())
        .and_then(|h| decode_info_hash(h).ok());

    let v2 = magnet_link
        .split('&')
        .find(|part| part.contains("xt=urn:btmh:"))
        .and_then(|part| part.split(':').next_back())
        .and_then(|h| decode_info_hash(h).ok());

    (v1, v2)
}
