// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use figment::providers::{Env, Format, Serialized};
use figment::{providers::Toml, Figment};
use sha1::{Digest, Sha1};
use tracing::{event as tracing_event, Level};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::app::FilePriority;
use crate::app::TorrentControlState;
use crate::theme::ThemeName;

use strum_macros::EnumCount;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, EnumIter, EnumCount)]
pub enum TorrentSortColumn {
    Name,
    #[default]
    Up,
    Down,
    Progress,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, EnumIter, EnumCount)]
pub enum PeerSortColumn {
    Flags,
    Completed,
    Address,
    Client,
    Action,
    #[default]
    #[serde(alias = "TotalUL")]
    UL,
    #[serde(alias = "TotalDL")]
    DL,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RssAddedVia {
    Auto,
    #[default]
    Manual,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct RssFeed {
    pub url: String,
    pub enabled: bool,
}

impl Default for RssFeed {
    fn default() -> Self {
        Self {
            url: String::new(),
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct RssFilter {
    #[serde(alias = "regex")]
    pub query: String,
    pub mode: RssFilterMode,
    pub enabled: bool,
}

impl Default for RssFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: RssFilterMode::Fuzzy,
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RssFilterMode {
    #[default]
    Fuzzy,
    Regex,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct RssSettings {
    pub enabled: bool,
    pub poll_interval_secs: u64,
    pub max_preview_items: usize,
    pub feeds: Vec<RssFeed>,
    pub filters: Vec<RssFilter>,
}

impl Default for RssSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_secs: 900,
            max_preview_items: 500,
            feeds: Vec::new(),
            filters: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RssHistoryEntry {
    pub dedupe_key: String,
    pub info_hash: Option<String>,
    pub guid: Option<String>,
    pub link: Option<String>,
    pub title: String,
    pub source: Option<String>,
    pub date_iso: String,
    pub added_via: RssAddedVia,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct FeedSyncError {
    pub message: String,
    pub occurred_at_iso: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub client_id: String,
    pub client_port: u16,
    pub torrents: Vec<TorrentSettings>,
    pub lifetime_downloaded: u64,
    pub lifetime_uploaded: u64,
    pub private_client: bool,
    pub torrent_sort_column: TorrentSortColumn,
    pub torrent_sort_direction: SortDirection,
    pub peer_sort_column: PeerSortColumn,
    pub peer_sort_direction: SortDirection,
    pub ui_theme: ThemeName,
    pub watch_folder: Option<PathBuf>,
    pub default_download_folder: Option<PathBuf>,
    pub max_connected_peers: usize,
    pub bootstrap_nodes: Vec<String>,
    pub global_download_limit_bps: u64,
    pub global_upload_limit_bps: u64,
    pub max_concurrent_validations: usize,
    pub connection_attempt_permits: usize,
    pub resource_limit_override: Option<usize>,
    pub upload_slots: usize,
    pub peer_upload_in_flight_limit: usize,
    pub tracker_fallback_interval_secs: u64,
    pub client_leeching_fallback_interval_secs: u64,
    pub output_status_interval: u64,
    pub rss: RssSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_port: 6681,
            torrents: Vec::new(),
            watch_folder: None,
            default_download_folder: None,
            lifetime_downloaded: 0,
            lifetime_uploaded: 0,
            private_client: false,
            global_download_limit_bps: 0,
            global_upload_limit_bps: 0,
            torrent_sort_column: TorrentSortColumn::default(),
            torrent_sort_direction: SortDirection::default(),
            peer_sort_column: PeerSortColumn::default(),
            peer_sort_direction: SortDirection::default(),
            ui_theme: ThemeName::default(),
            max_connected_peers: 2000,
            bootstrap_nodes: vec![
                "router.utorrent.com:6881".to_string(),
                "router.bittorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
                "dht.libtorrent.org:25401".to_string(),
                "router.cococorp.de:6881".to_string(),
            ],
            max_concurrent_validations: 64,
            resource_limit_override: None,
            connection_attempt_permits: 50,
            upload_slots: 8,
            peer_upload_in_flight_limit: 4,
            tracker_fallback_interval_secs: 1800,
            client_leeching_fallback_interval_secs: 60,
            output_status_interval: 0,
            rss: RssSettings::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
#[serde(default)]
pub struct TorrentSettings {
    pub torrent_or_magnet: String,
    pub name: String,
    pub validation_status: bool,
    pub download_path: Option<PathBuf>,
    pub container_name: Option<String>,
    pub torrent_control_state: TorrentControlState,
    #[serde(with = "string_usize_map")]
    pub file_priorities: HashMap<usize, FilePriority>,
}

mod string_usize_map {
    use crate::app::FilePriority;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::collections::HashMap;
    use std::str::FromStr;

    pub fn serialize<S>(
        map: &HashMap<usize, FilePriority>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string_map: HashMap<String, FilePriority> =
            map.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        serde::Serialize::serialize(&string_map, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<usize, FilePriority>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map: HashMap<String, FilePriority> = HashMap::deserialize(deserializer)?;
        let mut result = HashMap::new();
        for (k, v) in string_map {
            let k_usize = usize::from_str(&k).map_err(serde::de::Error::custom)?;
            result.insert(k_usize, v);
        }
        Ok(result)
    }
}

const SHARED_CONFIG_DIR_ENV: &str = "SUPERSEEDR_SHARED_CONFIG_DIR";
const SHARED_HOST_ID_ENV: &str = "SUPERSEEDR_HOST_ID";
const EXTRA_WATCH_PATH_PREFIX: &str = "SUPERSEEDR_WATCH_PATH_";
const SHARED_TORRENT_SOURCE_PREFIX: &str = "shared:";

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
enum SharedPath {
    Absolute(PathBuf),
    Portable { root: String, relative: PathBuf },
}

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
#[serde(default)]
struct CatalogTorrentSettings {
    pub torrent_or_magnet: String,
    pub name: String,
    pub validation_status: bool,
    pub download_path: Option<SharedPath>,
    pub container_name: Option<String>,
    pub torrent_control_state: TorrentControlState,
    #[serde(with = "string_usize_map")]
    pub file_priorities: HashMap<usize, FilePriority>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
struct SharedSettingsConfig {
    pub client_id: String,
    pub lifetime_downloaded: u64,
    pub lifetime_uploaded: u64,
    pub private_client: bool,
    pub torrent_sort_column: TorrentSortColumn,
    pub torrent_sort_direction: SortDirection,
    pub peer_sort_column: PeerSortColumn,
    pub peer_sort_direction: SortDirection,
    pub ui_theme: ThemeName,
    pub default_download_folder: Option<SharedPath>,
    pub max_connected_peers: usize,
    pub bootstrap_nodes: Vec<String>,
    pub global_download_limit_bps: u64,
    pub global_upload_limit_bps: u64,
    pub max_concurrent_validations: usize,
    pub connection_attempt_permits: usize,
    pub resource_limit_override: Option<usize>,
    pub upload_slots: usize,
    pub peer_upload_in_flight_limit: usize,
    pub tracker_fallback_interval_secs: u64,
    pub client_leeching_fallback_interval_secs: u64,
    pub output_status_interval: u64,
    pub rss: RssSettings,
}

impl Default for SharedSettingsConfig {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            client_id: settings.client_id,
            lifetime_downloaded: settings.lifetime_downloaded,
            lifetime_uploaded: settings.lifetime_uploaded,
            private_client: settings.private_client,
            torrent_sort_column: settings.torrent_sort_column,
            torrent_sort_direction: settings.torrent_sort_direction,
            peer_sort_column: settings.peer_sort_column,
            peer_sort_direction: settings.peer_sort_direction,
            ui_theme: settings.ui_theme,
            default_download_folder: None,
            max_connected_peers: settings.max_connected_peers,
            bootstrap_nodes: settings.bootstrap_nodes,
            global_download_limit_bps: settings.global_download_limit_bps,
            global_upload_limit_bps: settings.global_upload_limit_bps,
            max_concurrent_validations: settings.max_concurrent_validations,
            connection_attempt_permits: settings.connection_attempt_permits,
            resource_limit_override: settings.resource_limit_override,
            upload_slots: settings.upload_slots,
            peer_upload_in_flight_limit: settings.peer_upload_in_flight_limit,
            tracker_fallback_interval_secs: settings.tracker_fallback_interval_secs,
            client_leeching_fallback_interval_secs: settings.client_leeching_fallback_interval_secs,
            output_status_interval: settings.output_status_interval,
            rss: settings.rss,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
struct CatalogConfig {
    pub torrents: Vec<CatalogTorrentSettings>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
struct HostConfig {
    pub client_id: Option<String>,
    pub client_port: u16,
    pub watch_folder: Option<PathBuf>,
    pub path_roots: HashMap<String, PathBuf>,
}

impl Default for HostConfig {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            client_id: None,
            client_port: settings.client_port,
            watch_folder: settings.watch_folder,
            path_roots: HashMap::new(),
        }
    }
}
#[derive(Clone, Debug)]
struct NormalConfigPaths {
    config_dir: PathBuf,
    settings_path: PathBuf,
    backup_dir: PathBuf,
}

#[derive(Clone, Debug)]
struct SharedConfigPaths {
    root_dir: PathBuf,
    settings_path: PathBuf,
    catalog_path: PathBuf,
    host_path: PathBuf,
    host_id: String,
}

#[derive(Clone, Debug)]
struct NormalConfigBackend {
    paths: NormalConfigPaths,
}

#[derive(Clone, Debug)]
struct SharedConfigBackend {
    paths: SharedConfigPaths,
}

#[derive(Clone, Debug)]
enum ConfigBackend {
    Normal(NormalConfigBackend),
    Shared(SharedConfigBackend),
}

#[derive(Clone, Debug)]
struct SharedConfigState {
    paths: SharedConfigPaths,
    settings_config: SharedSettingsConfig,
    catalog: CatalogConfig,
    host: HostConfig,
    resolved_settings: Settings,
    settings_fingerprint: Option<String>,
    catalog_fingerprint: Option<String>,
    host_fingerprint: Option<String>,
}

static SHARED_CONFIG_STATE: OnceLock<Mutex<Option<SharedConfigState>>> = OnceLock::new();

fn shared_config_state() -> &'static Mutex<Option<SharedConfigState>> {
    SHARED_CONFIG_STATE.get_or_init(|| Mutex::new(None))
}

impl CatalogTorrentSettings {
    fn from_settings(
        settings: &TorrentSettings,
        path_roots: &HashMap<String, PathBuf>,
        shared_root: &Path,
    ) -> Self {
        Self {
            torrent_or_magnet: encode_catalog_torrent_source(
                &settings.torrent_or_magnet,
                shared_root,
            ),
            name: settings.name.clone(),
            validation_status: settings.validation_status,
            download_path: settings
                .download_path
                .as_deref()
                .map(|path| encode_shared_path(path, path_roots)),
            container_name: settings.container_name.clone(),
            torrent_control_state: settings.torrent_control_state.clone(),
            file_priorities: settings.file_priorities.clone(),
        }
    }

    fn to_settings(
        &self,
        path_roots: &HashMap<String, PathBuf>,
        shared_root: &Path,
    ) -> io::Result<TorrentSettings> {
        Ok(TorrentSettings {
            torrent_or_magnet: decode_catalog_torrent_source(&self.torrent_or_magnet, shared_root),
            name: self.name.clone(),
            validation_status: self.validation_status,
            download_path: self
                .download_path
                .as_ref()
                .map(|path| {
                    resolve_shared_path(path, path_roots, &format!("torrent '{}'", self.name))
                })
                .transpose()?,
            container_name: self.container_name.clone(),
            torrent_control_state: self.torrent_control_state.clone(),
            file_priorities: self.file_priorities.clone(),
        })
    }
}

impl SharedSettingsConfig {
    fn from_settings(settings: &Settings, path_roots: &HashMap<String, PathBuf>) -> Self {
        Self {
            client_id: settings.client_id.clone(),
            lifetime_downloaded: settings.lifetime_downloaded,
            lifetime_uploaded: settings.lifetime_uploaded,
            private_client: settings.private_client,
            torrent_sort_column: settings.torrent_sort_column,
            torrent_sort_direction: settings.torrent_sort_direction,
            peer_sort_column: settings.peer_sort_column,
            peer_sort_direction: settings.peer_sort_direction,
            ui_theme: settings.ui_theme,
            default_download_folder: settings
                .default_download_folder
                .as_deref()
                .map(|path| encode_shared_path(path, path_roots)),
            max_connected_peers: settings.max_connected_peers,
            bootstrap_nodes: settings.bootstrap_nodes.clone(),
            global_download_limit_bps: settings.global_download_limit_bps,
            global_upload_limit_bps: settings.global_upload_limit_bps,
            max_concurrent_validations: settings.max_concurrent_validations,
            connection_attempt_permits: settings.connection_attempt_permits,
            resource_limit_override: settings.resource_limit_override,
            upload_slots: settings.upload_slots,
            peer_upload_in_flight_limit: settings.peer_upload_in_flight_limit,
            tracker_fallback_interval_secs: settings.tracker_fallback_interval_secs,
            client_leeching_fallback_interval_secs: settings.client_leeching_fallback_interval_secs,
            output_status_interval: settings.output_status_interval,
            rss: settings.rss.clone(),
        }
    }

    fn apply_to_settings(
        &self,
        settings: &mut Settings,
        path_roots: &HashMap<String, PathBuf>,
    ) -> io::Result<()> {
        settings.client_id = self.client_id.clone();
        settings.lifetime_downloaded = self.lifetime_downloaded;
        settings.lifetime_uploaded = self.lifetime_uploaded;
        settings.private_client = self.private_client;
        settings.torrent_sort_column = self.torrent_sort_column;
        settings.torrent_sort_direction = self.torrent_sort_direction;
        settings.peer_sort_column = self.peer_sort_column;
        settings.peer_sort_direction = self.peer_sort_direction;
        settings.ui_theme = self.ui_theme;
        settings.default_download_folder = self
            .default_download_folder
            .as_ref()
            .map(|path| resolve_shared_path(path, path_roots, "default_download_folder"))
            .transpose()?;
        settings.max_connected_peers = self.max_connected_peers;
        settings.bootstrap_nodes = self.bootstrap_nodes.clone();
        settings.global_download_limit_bps = self.global_download_limit_bps;
        settings.global_upload_limit_bps = self.global_upload_limit_bps;
        settings.max_concurrent_validations = self.max_concurrent_validations;
        settings.connection_attempt_permits = self.connection_attempt_permits;
        settings.resource_limit_override = self.resource_limit_override;
        settings.upload_slots = self.upload_slots;
        settings.peer_upload_in_flight_limit = self.peer_upload_in_flight_limit;
        settings.tracker_fallback_interval_secs = self.tracker_fallback_interval_secs;
        settings.client_leeching_fallback_interval_secs =
            self.client_leeching_fallback_interval_secs;
        settings.output_status_interval = self.output_status_interval;
        settings.rss = self.rss.clone();
        Ok(())
    }
}

impl CatalogConfig {
    fn from_settings(
        settings: &Settings,
        path_roots: &HashMap<String, PathBuf>,
        shared_root: &Path,
    ) -> Self {
        Self {
            torrents: settings
                .torrents
                .iter()
                .map(|torrent| {
                    CatalogTorrentSettings::from_settings(torrent, path_roots, shared_root)
                })
                .collect(),
        }
    }

    fn apply_to_settings(
        &self,
        settings: &mut Settings,
        path_roots: &HashMap<String, PathBuf>,
        shared_root: &Path,
    ) -> io::Result<()> {
        settings.torrents = self
            .torrents
            .iter()
            .map(|torrent| torrent.to_settings(path_roots, shared_root))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(())
    }
}

impl HostConfig {
    fn from_settings(
        settings: &Settings,
        existing_roots: &HashMap<String, PathBuf>,
        shared_client_id: &str,
    ) -> Self {
        Self {
            client_id: (settings.client_id != shared_client_id).then(|| settings.client_id.clone()),
            client_port: settings.client_port,
            watch_folder: settings.watch_folder.clone(),
            path_roots: existing_roots.clone(),
        }
    }

    fn apply_to_settings(&self, settings: &mut Settings) {
        if let Some(client_id) = &self.client_id {
            settings.client_id = client_id.clone();
        }
        settings.client_port = self.client_port;
        settings.watch_folder = self.watch_folder.clone();
    }
}
fn sanitize_host_id(raw: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_separator = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            sanitized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            sanitized.push('-');
            last_was_separator = true;
        }
    }

    sanitized.trim_matches('-').to_string()
}

fn shared_config_root() -> Option<PathBuf> {
    env::var_os(SHARED_CONFIG_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn sanitized_host_id_candidate(raw: &str) -> Option<String> {
    let sanitized = sanitize_host_id(raw);
    (!sanitized.is_empty()).then_some(sanitized)
}

fn resolve_host_id_from_sources(
    explicit_host_id: Option<String>,
    env_hostnames: Vec<String>,
    system_hostname: Option<String>,
) -> String {
    if let Some(host_id) = explicit_host_id
        .as_deref()
        .and_then(sanitized_host_id_candidate)
    {
        return host_id;
    }

    for hostname in env_hostnames {
        if let Some(host_id) = sanitized_host_id_candidate(&hostname) {
            return host_id;
        }
    }

    if let Some(host_id) = system_hostname
        .as_deref()
        .and_then(sanitized_host_id_candidate)
    {
        return host_id;
    }

    "default-host".to_string()
}

fn resolve_host_id() -> String {
    let explicit_host_id = env::var(SHARED_HOST_ID_ENV).ok();
    let env_hostnames = ["HOSTNAME", "COMPUTERNAME"]
        .into_iter()
        .filter_map(|key| env::var(key).ok())
        .collect();
    let system_hostname = sysinfo::System::host_name();

    resolve_host_id_from_sources(explicit_host_id, env_hostnames, system_hostname)
}

fn resolve_shared_config_paths() -> io::Result<Option<SharedConfigPaths>> {
    let Some(root_dir) = shared_config_root() else {
        return Ok(None);
    };
    let host_id = resolve_host_id();
    Ok(Some(SharedConfigPaths {
        settings_path: root_dir.join("settings.toml"),
        catalog_path: root_dir.join("catalog.toml"),
        host_path: root_dir.join("hosts").join(format!("{}.toml", host_id)),
        root_dir,
        host_id,
    }))
}

fn resolve_config_backend() -> io::Result<ConfigBackend> {
    if let Some(paths) = resolve_shared_config_paths()? {
        return Ok(ConfigBackend::Shared(SharedConfigBackend { paths }));
    }

    let (config_dir, _) = get_app_paths().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not resolve application config directory",
        )
    })?;
    Ok(ConfigBackend::Normal(NormalConfigBackend {
        paths: NormalConfigPaths {
            settings_path: config_dir.join("settings.toml"),
            backup_dir: config_dir.join("backups_settings_files"),
            config_dir,
        },
    }))
}

fn encode_shared_path(path: &Path, path_roots: &HashMap<String, PathBuf>) -> SharedPath {
    let mut matches: Vec<_> = path_roots
        .iter()
        .filter_map(|(root, base_path)| {
            path.strip_prefix(base_path).ok().map(|relative| {
                (
                    root.clone(),
                    relative.to_path_buf(),
                    base_path.components().count(),
                )
            })
        })
        .collect();

    matches.sort_by_key(|(_, _, component_count)| Reverse(*component_count));
    if let Some((root, relative, _)) = matches.into_iter().next() {
        SharedPath::Portable { root, relative }
    } else {
        SharedPath::Absolute(path.to_path_buf())
    }
}

fn resolve_shared_path(
    path: &SharedPath,
    path_roots: &HashMap<String, PathBuf>,
    context: &str,
) -> io::Result<PathBuf> {
    match path {
        SharedPath::Absolute(path) => Ok(path.clone()),
        SharedPath::Portable { root, relative } => {
            let base_path = path_roots.get(root).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "Missing host path root '{}' while resolving {}",
                        root, context
                    ),
                )
            })?;
            Ok(base_path.join(relative))
        }
    }
}

fn shared_path_root(path: &SharedPath) -> Option<&str> {
    match path {
        SharedPath::Absolute(_) => None,
        SharedPath::Portable { root, .. } => Some(root.as_str()),
    }
}

fn collect_required_path_roots(
    settings_config: &SharedSettingsConfig,
    catalog: &CatalogConfig,
) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();

    if let Some(root) = settings_config
        .default_download_folder
        .as_ref()
        .and_then(shared_path_root)
    {
        roots.insert(root.to_string());
    }

    for torrent in &catalog.torrents {
        if let Some(root) = torrent.download_path.as_ref().and_then(shared_path_root) {
            roots.insert(root.to_string());
        }
    }

    roots
}

fn inferred_shared_data_root(paths: &SharedConfigPaths) -> PathBuf {
    paths
        .root_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths.root_dir.clone())
}

fn bootstrap_shared_host_config(
    paths: &SharedConfigPaths,
    settings_config: &SharedSettingsConfig,
    catalog: &CatalogConfig,
) -> io::Result<HostConfig> {
    let mut host = HostConfig::default();
    let inferred_root = inferred_shared_data_root(paths);

    for root in collect_required_path_roots(settings_config, catalog) {
        host.path_roots.insert(root, inferred_root.clone());
    }

    write_toml_atomically(&paths.host_path, &host)?;
    tracing_event!(
        Level::INFO,
        "Bootstrapped shared host config at {:?}",
        paths.host_path
    );

    Ok(host)
}
fn portable_relative_path_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn shared_relative_path_to_pathbuf(relative: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for segment in relative.split(['/', '\\']) {
        if !segment.is_empty() {
            path.push(segment);
        }
    }
    path
}

fn encode_catalog_torrent_source(source: &str, shared_root: &Path) -> String {
    if source.starts_with("magnet:") {
        return source.to_string();
    }

    let path = Path::new(source);
    if let Ok(relative) = path.strip_prefix(shared_root) {
        return format!(
            "{}{}",
            SHARED_TORRENT_SOURCE_PREFIX,
            portable_relative_path_string(relative)
        );
    }

    source.to_string()
}

fn decode_catalog_torrent_source(source: &str, shared_root: &Path) -> String {
    let Some(relative) = source.strip_prefix(SHARED_TORRENT_SOURCE_PREFIX) else {
        return source.to_string();
    };

    shared_root
        .join(shared_relative_path_to_pathbuf(relative))
        .to_string_lossy()
        .to_string()
}

fn apply_env_overrides(settings: &Settings) -> io::Result<Settings> {
    Figment::from(Serialized::defaults(settings.clone()))
        .merge(Env::prefixed("SUPERSEEDR_"))
        .extract::<Settings>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_toml_or_default<T>(path: &Path) -> io::Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let content = fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn fingerprint_for_path(path: &Path) -> io::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path)?;
    Ok(Some(hex::encode(Sha1::digest(bytes))))
}

fn ensure_fingerprint_matches(
    path: &Path,
    expected: &Option<String>,
    label: &str,
) -> io::Result<()> {
    let current = fingerprint_for_path(path)?;
    if &current != expected {
        return Err(io::Error::other(format!(
            "{} changed on disk at {:?}; reload required before saving",
            label, path
        )));
    }
    Ok(())
}

fn write_toml_atomically<T: Serialize>(path: &Path, value: &T) -> io::Result<Option<String>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(value).map_err(io::Error::other)?;
    let tmp_path = path.with_extension("toml.tmp");
    fs::write(&tmp_path, &content)?;
    fs::rename(&tmp_path, path)?;
    Ok(Some(hex::encode(Sha1::digest(content.as_bytes()))))
}

fn clear_shared_config_state() {
    if let Ok(mut guard) = shared_config_state().lock() {
        *guard = None;
    }
}

fn first_run_settings() -> Settings {
    let mut settings = Settings::default();
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(dl_dir) = user_dirs.download_dir() {
            settings.default_download_folder = Some(dl_dir.to_path_buf());
        }
    }
    settings
}

impl NormalConfigBackend {
    fn load_settings(&self) -> io::Result<Settings> {
        if !self.paths.settings_path.exists() {
            tracing_event!(
                Level::INFO,
                "No settings found. Performing first-run setup."
            );
            let settings = first_run_settings();
            self.save_settings(&settings)?;
            return Ok(settings);
        }

        tracing_event!(
            Level::INFO,
            "Found existing settings at: {:?}",
            self.paths.settings_path
        );

        Figment::new()
            .merge(Toml::file(&self.paths.settings_path))
            .merge(Env::prefixed("SUPERSEEDR_"))
            .extract::<Settings>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        fs::create_dir_all(&self.paths.backup_dir)?;

        let now = chrono::Local::now();
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let backup_path = self
            .paths
            .backup_dir
            .join(format!("settings_{}.toml", timestamp));

        let content = toml::to_string_pretty(settings).map_err(io::Error::other)?;
        let temp_file_path = self.paths.config_dir.join("settings.toml.tmp");
        fs::write(&temp_file_path, &content)?;
        fs::rename(&temp_file_path, &self.paths.settings_path)?;
        fs::write(backup_path, content)?;
        cleanup_old_backups(&self.paths.backup_dir, 64)?;

        Ok(())
    }
}

impl SharedConfigBackend {
    fn load_settings(&self) -> io::Result<Settings> {
        let settings_config: SharedSettingsConfig =
            read_toml_or_default(&self.paths.settings_path)?;
        let catalog: CatalogConfig = read_toml_or_default(&self.paths.catalog_path)?;
        let host = if self.paths.host_path.exists() {
            read_toml_or_default(&self.paths.host_path)?
        } else {
            bootstrap_shared_host_config(&self.paths, &settings_config, &catalog)?
        };

        let mut settings = Settings::default();
        settings_config.apply_to_settings(&mut settings, &host.path_roots)?;
        catalog.apply_to_settings(&mut settings, &host.path_roots, &self.paths.root_dir)?;
        host.apply_to_settings(&mut settings);
        let resolved_settings = apply_env_overrides(&settings)?;
        let settings_fingerprint = fingerprint_for_path(&self.paths.settings_path)?;
        let catalog_fingerprint = fingerprint_for_path(&self.paths.catalog_path)?;
        let host_fingerprint = fingerprint_for_path(&self.paths.host_path)?;

        let mut guard = shared_config_state()
            .lock()
            .map_err(|_| io::Error::other("Shared config state lock poisoned"))?;
        *guard = Some(SharedConfigState {
            paths: self.paths.clone(),
            settings_config,
            catalog,
            host,
            resolved_settings: resolved_settings.clone(),
            settings_fingerprint,
            catalog_fingerprint,
            host_fingerprint,
        });

        Ok(resolved_settings)
    }

    fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        let mut guard = shared_config_state()
            .lock()
            .map_err(|_| io::Error::other("Shared config state lock poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| io::Error::other("Shared config mode was not loaded before save"))?;

        ensure_fingerprint_matches(
            &state.paths.settings_path,
            &state.settings_fingerprint,
            "Shared settings",
        )?;
        ensure_fingerprint_matches(
            &state.paths.catalog_path,
            &state.catalog_fingerprint,
            "Shared catalog",
        )?;
        ensure_fingerprint_matches(
            &state.paths.host_path,
            &state.host_fingerprint,
            "Shared host config",
        )?;

        let mut next_settings_config =
            SharedSettingsConfig::from_settings(settings, &state.host.path_roots);
        if state.host.client_id.is_some() {
            next_settings_config.client_id = state.settings_config.client_id.clone();
        }
        let next_catalog =
            CatalogConfig::from_settings(settings, &state.host.path_roots, &state.paths.root_dir);
        let next_host = HostConfig::from_settings(
            settings,
            &state.host.path_roots,
            &next_settings_config.client_id,
        );

        if next_settings_config != state.settings_config || state.settings_fingerprint.is_none() {
            state.settings_fingerprint =
                write_toml_atomically(&self.paths.settings_path, &next_settings_config)?;
            state.settings_config = next_settings_config;
        }

        if next_catalog != state.catalog || state.catalog_fingerprint.is_none() {
            state.catalog_fingerprint =
                write_toml_atomically(&self.paths.catalog_path, &next_catalog)?;
            state.catalog = next_catalog;
        }

        if next_host != state.host || state.host_fingerprint.is_none() {
            state.host_fingerprint = write_toml_atomically(&self.paths.host_path, &next_host)?;
            state.host = next_host;
        }

        state.resolved_settings = settings.clone();
        Ok(())
    }
}

impl ConfigBackend {
    fn load_settings(&self) -> io::Result<Settings> {
        match self {
            ConfigBackend::Normal(backend) => {
                clear_shared_config_state();
                backend.load_settings()
            }
            ConfigBackend::Shared(backend) => {
                tracing_event!(
                    Level::INFO,
                    "Using shared config root {:?} with host id {}",
                    backend.paths.root_dir,
                    backend.paths.host_id
                );
                backend.load_settings()
            }
        }
    }

    fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        match self {
            ConfigBackend::Normal(backend) => backend.save_settings(settings),
            ConfigBackend::Shared(backend) => backend.save_settings(settings),
        }
    }
}

pub fn get_app_paths() -> Option<(PathBuf, PathBuf)> {
    if let Some(proj_dirs) = ProjectDirs::from("com", "github", "jagalite.superseedr") {
        let config_dir = proj_dirs.config_dir().to_path_buf();
        let data_dir = proj_dirs.data_local_dir().to_path_buf();

        fs::create_dir_all(&config_dir).ok()?;
        fs::create_dir_all(&data_dir).ok()?;

        Some((config_dir, data_dir))
    } else {
        None
    }
}

pub fn is_shared_config_mode() -> bool {
    shared_config_root().is_some()
}

pub fn shared_settings_path() -> Option<PathBuf> {
    resolve_shared_config_paths()
        .ok()
        .flatten()
        .map(|paths| paths.settings_path)
}

pub fn shared_torrents_path() -> Option<PathBuf> {
    shared_config_root().map(|root| root.join("torrents"))
}

pub fn shared_torrent_file_path(info_hash: &[u8]) -> Option<PathBuf> {
    shared_torrents_path().map(|path| path.join(format!("{}.torrent", hex::encode(info_hash))))
}

pub fn shared_config_watch_paths() -> Vec<PathBuf> {
    let Some(paths) = resolve_shared_config_paths().ok().flatten() else {
        return Vec::new();
    };

    let mut watch_paths = vec![paths.root_dir.clone()];
    if let Some(host_dir) = paths.host_path.parent() {
        if host_dir != paths.root_dir {
            watch_paths.push(host_dir.to_path_buf());
        }
    }
    watch_paths
}

pub fn is_shared_config_path(path: &Path) -> bool {
    resolve_shared_config_paths()
        .ok()
        .flatten()
        .is_some_and(|paths| {
            path == paths.settings_path || path == paths.catalog_path || path == paths.host_path
        })
}

pub fn resolve_command_watch_path(settings: &Settings) -> Option<PathBuf> {
    settings
        .watch_folder
        .clone()
        .or_else(|| get_watch_path().map(|(watch_path, _)| watch_path))
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn resolve_additional_watch_paths_from_sources<I, K, V>(vars: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let mut indexed_paths = vars
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.into();
            let value = value.into();
            let key = key.to_string_lossy();
            let suffix = key.strip_prefix(EXTRA_WATCH_PATH_PREFIX)?;

            if suffix.is_empty() || value.is_empty() {
                return None;
            }

            let index = suffix.parse::<usize>().ok();
            Some((index, suffix.to_string(), PathBuf::from(value)))
        })
        .collect::<Vec<_>>();

    indexed_paths.sort_by(|left, right| {
        left.0
            .unwrap_or(usize::MAX)
            .cmp(&right.0.unwrap_or(usize::MAX))
            .then_with(|| left.1.cmp(&right.1))
    });

    let mut paths = Vec::new();
    for (_, _, path) in indexed_paths {
        push_unique_path(&mut paths, path);
    }
    paths
}

pub fn additional_watch_paths() -> Vec<PathBuf> {
    resolve_additional_watch_paths_from_sources(env::vars_os())
}

pub fn configured_watch_paths(settings: &Settings) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(path) = settings.watch_folder.clone() {
        push_unique_path(&mut paths, path);
    }

    for path in additional_watch_paths() {
        push_unique_path(&mut paths, path);
    }

    if paths.is_empty() {
        if let Some((watch_path, _)) = get_watch_path() {
            push_unique_path(&mut paths, watch_path);
        }
    }

    paths
}

pub fn get_watch_path() -> Option<(PathBuf, PathBuf)> {
    if let Some((_, base_path)) = get_app_paths() {
        let watch_path = base_path.join("watch_files");
        let processed_path = base_path.join("processed_files");
        Some((watch_path, processed_path))
    } else {
        None
    }
}

pub fn create_watch_directories() -> io::Result<()> {
    if let Some((watch_path, processed_path)) = get_watch_path() {
        fs::create_dir_all(&watch_path)?;
        fs::create_dir_all(&processed_path)?;
    }

    Ok(())
}

pub fn load_settings() -> io::Result<Settings> {
    resolve_config_backend()?.load_settings()
}

pub fn save_settings(settings: &Settings) -> io::Result<()> {
    resolve_config_backend()?.save_settings(settings)
}
fn cleanup_old_backups(backup_dir: &PathBuf, limit: usize) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(backup_dir)?
        .filter_map(|res| res.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("settings_") && s.ends_with(".toml"))
                .unwrap_or(false)
        })
        .collect();

    if entries.len() > limit {
        entries.sort();
        for path in entries.iter().take(entries.len() - limit) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::providers::{Format, Toml};
    use figment::Figment;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_full_settings_parsing() {
        let toml_str = r#"
            client_id = "test-client-id-123"
            client_port = 12345
            lifetime_downloaded = 1000
            lifetime_uploaded = 2000

            torrent_sort_column = "Name"
            torrent_sort_direction = "Descending"
            peer_sort_column = "Address"
            peer_sort_direction = "Ascending"

            watch_folder = "/path/to/watch"
            default_download_folder = "/path/to/download"

            max_connected_peers = 500
            global_download_limit_bps = 102400
            global_upload_limit_bps = 51200

            max_concurrent_validations = 32
            connection_attempt_permits = 25
            resource_limit_override = 1024

            upload_slots = 10
            peer_upload_in_flight_limit = 2

            tracker_fallback_interval_secs = 3600
            client_leeching_fallback_interval_secs = 120

            bootstrap_nodes = [
                "node1.com:1234",
                "node2.com:5678"
            ]

            [[torrents]]
            torrent_or_magnet = "magnet:?xt=urn:btih:..."
            name = "My Test Torrent"
            validation_status = true
            download_path = "/downloads/my_test_torrent"

            [[torrents]]
            torrent_or_magnet = "magnet:?xt=urn:btih:other"
            name = "Another Torrent"
            validation_status = false
            download_path = "/downloads/another"
            torrent_control_state = "Paused"
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse full TOML string");

        assert_eq!(settings.client_id, "test-client-id-123");
        assert_eq!(settings.client_port, 12345);
        assert_eq!(settings.lifetime_downloaded, 1000);
        assert_eq!(settings.global_upload_limit_bps, 51200);
        assert_eq!(settings.torrent_sort_column, TorrentSortColumn::Name);
        assert_eq!(settings.torrent_sort_direction, SortDirection::Descending);
        assert_eq!(settings.peer_sort_column, PeerSortColumn::Address);
        assert_eq!(settings.watch_folder, Some(PathBuf::from("/path/to/watch")));
        assert_eq!(settings.resource_limit_override, Some(1024));
        assert_eq!(
            settings.bootstrap_nodes,
            vec!["node1.com:1234", "node2.com:5678"]
        );
        assert_eq!(settings.torrents.len(), 2);
        assert_eq!(settings.torrents[0].name, "My Test Torrent");
        assert!(settings.torrents[0].validation_status);
        assert_eq!(
            settings.torrents[0].download_path,
            Some(PathBuf::from("/downloads/my_test_torrent"))
        );
        assert_eq!(settings.torrents[1].name, "Another Torrent");
        assert_eq!(
            settings.torrents[1].torrent_control_state,
            TorrentControlState::Paused
        );
    }

    #[test]
    fn test_partial_settings_override() {
        let toml_str = r#"
            client_port = 9999
            global_upload_limit_bps = 50000

            [[torrents]]
            name = "Partial Torrent"
            download_path = "/partial/path"
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse partial TOML string");

        let default_settings = Settings::default();

        assert_eq!(settings.client_port, 9999);
        assert_eq!(settings.global_upload_limit_bps, 50000);
        assert_eq!(settings.client_id, default_settings.client_id);
        assert_eq!(
            settings.max_connected_peers,
            default_settings.max_connected_peers
        );
        assert_eq!(
            settings.torrent_sort_column,
            default_settings.torrent_sort_column
        );
        assert_eq!(settings.torrents.len(), 1);
        assert_eq!(settings.torrents[0].name, "Partial Torrent");
        assert_eq!(
            settings.torrents[0].download_path,
            Some(PathBuf::from("/partial/path"))
        );
        assert_eq!(settings.torrents[0].torrent_or_magnet, "");
        assert!(!settings.torrents[0].validation_status);
        assert_eq!(
            settings.torrents[0].torrent_control_state,
            TorrentControlState::default()
        );
    }

    #[test]
    fn test_default_settings() {
        let toml_str = "";

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse empty string");

        let default_settings = Settings::default();

        assert_eq!(settings.client_id, default_settings.client_id);
        assert_eq!(settings.client_port, 6681);
        assert_eq!(settings.lifetime_downloaded, 0);
        assert_eq!(settings.global_upload_limit_bps, 0);
        assert_eq!(settings.torrent_sort_column, TorrentSortColumn::Up);
        assert_eq!(settings.peer_sort_direction, SortDirection::Ascending);
        assert!(settings.watch_folder.is_none());
        assert_eq!(settings.max_connected_peers, 2000);
        assert_eq!(settings.bootstrap_nodes, default_settings.bootstrap_nodes);
        assert!(settings.torrents.is_empty());
    }

    #[test]
    fn test_invalid_ui_theme_type_does_not_fail_settings_parse() {
        let toml_str = r#"
            client_id = "theme-type-regression"
            client_port = 7777
            ui_theme = 123
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Settings parsing should not fail for non-string ui_theme");

        assert_eq!(settings.client_id, "theme-type-regression");
        assert_eq!(settings.client_port, 7777);
        assert_eq!(
            settings.ui_theme,
            ThemeName::default(),
            "Invalid ui_theme type should safely fallback to default"
        );
    }

    #[test]
    fn test_rss_filter_legacy_regex_key_is_accepted() {
        let toml_str = r#"
            [rss]
            enabled = true
            poll_interval_secs = 300
            max_preview_items = 50

            [[rss.filters]]
            regex = "ubuntu"
            enabled = true
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Settings parsing should accept legacy rss.filters.regex key");

        assert_eq!(settings.rss.filters.len(), 1);
        assert_eq!(settings.rss.filters[0].query, "ubuntu");
        assert!(matches!(settings.rss.filters[0].mode, RssFilterMode::Fuzzy));
        assert!(settings.rss.filters[0].enabled);
    }

    #[test]
    fn test_rss_filter_mode_regex_is_parsed() {
        let toml_str = r#"
            [rss]
            enabled = true

            [[rss.filters]]
            query = "series\\s+alpha"
            mode = "regex"
            enabled = true
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Settings parsing should accept rss.filters.mode");

        assert_eq!(settings.rss.filters.len(), 1);
        assert!(matches!(settings.rss.filters[0].mode, RssFilterMode::Regex));
    }

    #[test]
    fn test_invalid_torrent_state_parsing() {
        let toml_str = r#"
            [[torrents]]
            name = "Invalid Torrent"
            download_path = "/invalid/path"
            torrent_control_state = "UNKNOWN"
        "#;

        let result: Result<Settings, figment::Error> =
            Figment::new().merge(Toml::string(toml_str)).extract();

        assert!(
            result.is_err(),
            "Parsing should fail with an invalid enum variant"
        );

        if let Err(e) = result {
            let error_string = e.to_string();
            assert!(
                error_string.contains("UNKNOWN"),
                "Error message should mention the invalid variant 'UNKNOWN'"
            );
            assert!(
                error_string.contains("torrent_control_state"),
                "Error message should mention the field 'torrent_control_state'"
            );
        }
    }

    #[test]
    fn test_shared_path_round_trip_through_roots() {
        let mut roots = HashMap::new();
        roots.insert("media".to_string(), PathBuf::from("/mnt/nas"));

        let shared = encode_shared_path(Path::new("/mnt/nas/downloads/alpha"), &roots);
        assert_eq!(
            shared,
            SharedPath::Portable {
                root: "media".to_string(),
                relative: PathBuf::from("downloads/alpha"),
            }
        );

        let resolved = resolve_shared_path(&shared, &roots, "test path").expect("resolve path");
        assert_eq!(resolved, PathBuf::from("/mnt/nas/downloads/alpha"));
    }

    #[test]
    fn test_resolve_shared_path_reports_missing_root() {
        let err = resolve_shared_path(
            &SharedPath::Portable {
                root: "media".to_string(),
                relative: PathBuf::from("downloads/alpha"),
            },
            &HashMap::new(),
            "default_download_folder",
        )
        .expect_err("missing root should fail");

        assert!(err.to_string().contains("Missing host path root 'media'"));
    }

    #[test]
    fn test_resolve_host_id_uses_system_hostname_fallback() {
        let resolved =
            resolve_host_id_from_sources(None, Vec::new(), Some("MacBook Pro.local".to_string()));

        assert_eq!(resolved, "macbook-pro.local");
    }

    #[test]
    fn test_resolve_host_id_prefers_explicit_override() {
        let resolved = resolve_host_id_from_sources(
            Some("Custom Laptop".to_string()),
            vec!["IgnoredHost".to_string()],
            Some("IgnoredSystem".to_string()),
        );

        assert_eq!(resolved, "custom-laptop");
    }

    #[test]
    fn test_shared_torrent_source_round_trip() {
        let shared_root = Path::new("/shared-root");
        let absolute = "/shared-root/torrents/0123456789abcdef0123456789abcdef01234567.torrent";
        let encoded = encode_catalog_torrent_source(absolute, shared_root);
        assert_eq!(
            encoded,
            "shared:torrents/0123456789abcdef0123456789abcdef01234567.torrent"
        );
        let decoded = decode_catalog_torrent_source(&encoded, shared_root);
        assert_eq!(PathBuf::from(decoded), PathBuf::from(absolute));
    }

    #[test]
    fn test_catalog_and_host_merge_into_runtime_settings() {
        let mut roots = HashMap::new();
        roots.insert("media".to_string(), PathBuf::from("/mnt/nas"));

        let shared_settings = SharedSettingsConfig {
            client_id: "shared-id".to_string(),
            default_download_folder: Some(SharedPath::Portable {
                root: "media".to_string(),
                relative: PathBuf::from("downloads"),
            }),
            global_download_limit_bps: 1234,
            ..SharedSettingsConfig::default()
        };
        let catalog = CatalogConfig {
            torrents: vec![CatalogTorrentSettings {
                name: "Shared Collection".to_string(),
                download_path: Some(SharedPath::Portable {
                    root: "media".to_string(),
                    relative: PathBuf::from("downloads/shared"),
                }),
                ..CatalogTorrentSettings::default()
            }],
        };
        let host = HostConfig {
            client_id: Some("host-a".to_string()),
            client_port: 7777,
            watch_folder: Some(PathBuf::from("/watch")),
            path_roots: roots.clone(),
        };

        let mut settings = Settings::default();
        shared_settings
            .apply_to_settings(&mut settings, &host.path_roots)
            .expect("apply shared settings");
        catalog
            .apply_to_settings(&mut settings, &host.path_roots, Path::new("/shared-root"))
            .expect("apply catalog");
        host.apply_to_settings(&mut settings);

        assert_eq!(settings.client_id, "host-a");
        assert_eq!(settings.client_port, 7777);
        assert_eq!(settings.watch_folder, Some(PathBuf::from("/watch")));
        assert_eq!(
            settings.default_download_folder,
            Some(PathBuf::from("/mnt/nas/downloads"))
        );
        assert_eq!(settings.global_download_limit_bps, 1234);
        assert_eq!(
            settings.torrents[0].download_path,
            Some(PathBuf::from("/mnt/nas/downloads/shared"))
        );
    }

    #[test]
    fn test_host_override_client_id_wins_over_shared_default() {
        let shared_settings = SharedSettingsConfig {
            client_id: "shared-id".to_string(),
            ..SharedSettingsConfig::default()
        };
        let host = HostConfig {
            client_id: Some("host-id".to_string()),
            ..HostConfig::default()
        };

        let mut settings = Settings::default();
        shared_settings
            .apply_to_settings(&mut settings, &HashMap::new())
            .expect("apply shared settings");
        host.apply_to_settings(&mut settings);

        assert_eq!(settings.client_id, "host-id");
    }

    #[test]
    fn test_fingerprint_detection_catches_stale_write() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("catalog.toml");
        fs::write(&path, "value = 1\n").expect("write file");
        let fingerprint = fingerprint_for_path(&path).expect("fingerprint");
        fs::write(&path, "value = 2\n").expect("rewrite file");

        let err = ensure_fingerprint_matches(&path, &fingerprint, "Shared catalog")
            .expect_err("stale write should fail");
        assert!(err.to_string().contains("reload required"));
    }

    #[test]
    fn test_write_toml_atomically_writes_file() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("host.toml");
        let host = HostConfig {
            client_id: Some("host-a".to_string()),
            ..HostConfig::default()
        };

        let fingerprint = write_toml_atomically(&path, &host).expect("write toml");
        assert!(path.exists());
        assert!(fingerprint.is_some());
    }

    #[test]
    fn test_normal_backend_round_trips_settings() {
        let dir = tempdir().expect("create tempdir");
        let backend = NormalConfigBackend {
            paths: NormalConfigPaths {
                config_dir: dir.path().to_path_buf(),
                settings_path: dir.path().join("settings.toml"),
                backup_dir: dir.path().join("backups_settings_files"),
            },
        };
        let settings = Settings {
            client_id: "unit-host".to_string(),
            client_port: 7777,
            global_download_limit_bps: 1234,
            ..Settings::default()
        };

        backend.save_settings(&settings).expect("save settings");
        let loaded = backend.load_settings().expect("load settings");

        assert_eq!(loaded.client_id, "unit-host");
        assert_eq!(loaded.client_port, 7777);
        assert_eq!(loaded.global_download_limit_bps, 1234);
        assert!(backend.paths.settings_path.exists());
    }

    #[test]
    fn test_shared_backend_routes_shared_and_host_fields() {
        clear_shared_config_state();
        let dir = tempdir().expect("create tempdir");
        let backend = SharedConfigBackend {
            paths: SharedConfigPaths {
                root_dir: dir.path().to_path_buf(),
                settings_path: dir.path().join("settings.toml"),
                catalog_path: dir.path().join("catalog.toml"),
                host_path: dir.path().join("hosts").join("node-a.toml"),
                host_id: "node-a".to_string(),
            },
        };
        let shared_torrent_path = backend
            .paths
            .root_dir
            .join("torrents")
            .join("0123456789abcdef0123456789abcdef01234567.torrent");

        let mut loaded = backend.load_settings().expect("load shared settings");
        loaded.client_id = "shared-node".to_string();
        loaded.client_port = 9090;
        loaded.watch_folder = Some(PathBuf::from("/watch"));
        loaded.global_upload_limit_bps = 4321;
        loaded.default_download_folder = Some(PathBuf::from("/shared/downloads"));
        loaded.torrents.push(TorrentSettings {
            torrent_or_magnet: shared_torrent_path.to_string_lossy().to_string(),
            name: "Library Item".to_string(),
            download_path: Some(PathBuf::from("/shared/downloads/library-item")),
            ..TorrentSettings::default()
        });

        backend
            .save_settings(&loaded)
            .expect("save shared settings");
        let reloaded = backend.load_settings().expect("reload shared settings");

        let settings_contents =
            fs::read_to_string(&backend.paths.settings_path).expect("read settings file");
        let host_contents = fs::read_to_string(&backend.paths.host_path).expect("read host file");
        let catalog_contents =
            fs::read_to_string(&backend.paths.catalog_path).expect("read catalog file");

        assert!(host_contents.contains("client_port = 9090"));
        assert!(!host_contents.contains("client_id"));
        assert!(host_contents.contains("watch_folder = \"/watch\""));
        assert!(settings_contents.contains("client_id = \"shared-node\""));
        assert!(settings_contents.contains("global_upload_limit_bps = 4321"));
        assert!(settings_contents.contains("default_download_folder = \"/shared/downloads\""));
        assert!(catalog_contents.contains("[[torrents]]"));
        assert!(catalog_contents.contains("name = \"Library Item\""));
        assert!(catalog_contents.contains("torrent_or_magnet = \"shared:torrents/0123456789abcdef0123456789abcdef01234567.torrent\""));
        assert!(!catalog_contents.contains("global_upload_limit_bps"));
        assert_eq!(
            reloaded.torrents[0].torrent_or_magnet,
            shared_torrent_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn test_shared_backend_bootstraps_missing_host_file_with_inferred_roots() {
        clear_shared_config_state();
        let dir = tempdir().expect("create tempdir");
        let shared_root = dir.path().join("superseedr-config");
        let backend = SharedConfigBackend {
            paths: SharedConfigPaths {
                root_dir: shared_root.clone(),
                settings_path: shared_root.join("settings.toml"),
                catalog_path: shared_root.join("catalog.toml"),
                host_path: shared_root.join("hosts").join("windows-node.toml"),
                host_id: "windows-node".to_string(),
            },
        };

        fs::create_dir_all(&backend.paths.root_dir).expect("create shared root");
        write_toml_atomically(
            &backend.paths.settings_path,
            &SharedSettingsConfig {
                default_download_folder: Some(SharedPath::Portable {
                    root: "media".to_string(),
                    relative: PathBuf::new(),
                }),
                ..SharedSettingsConfig::default()
            },
        )
        .expect("seed shared settings");

        let loaded = backend.load_settings().expect("load shared settings");
        let host_contents = fs::read_to_string(&backend.paths.host_path).expect("read host file");

        assert_eq!(
            loaded.default_download_folder,
            Some(dir.path().to_path_buf())
        );
        assert!(host_contents.contains("[path_roots]"));
        assert!(host_contents.contains("media = "));
    }

    #[test]
    fn test_shared_backend_preserves_shared_client_id_when_host_override_exists() {
        clear_shared_config_state();
        let dir = tempdir().expect("create tempdir");
        let backend = SharedConfigBackend {
            paths: SharedConfigPaths {
                root_dir: dir.path().to_path_buf(),
                settings_path: dir.path().join("settings.toml"),
                catalog_path: dir.path().join("catalog.toml"),
                host_path: dir.path().join("hosts").join("node-a.toml"),
                host_id: "node-a".to_string(),
            },
        };

        write_toml_atomically(
            &backend.paths.settings_path,
            &SharedSettingsConfig {
                client_id: "shared-default".to_string(),
                ..SharedSettingsConfig::default()
            },
        )
        .expect("seed shared settings");
        write_toml_atomically(
            &backend.paths.host_path,
            &HostConfig {
                client_id: Some("host-override".to_string()),
                ..HostConfig::default()
            },
        )
        .expect("seed host config");

        let mut loaded = backend.load_settings().expect("load shared settings");
        assert_eq!(loaded.client_id, "host-override");

        loaded.global_download_limit_bps = 9876;
        backend
            .save_settings(&loaded)
            .expect("save shared settings");

        let settings_contents =
            fs::read_to_string(&backend.paths.settings_path).expect("read settings file");
        let host_contents = fs::read_to_string(&backend.paths.host_path).expect("read host file");

        assert!(settings_contents.contains("client_id = \"shared-default\""));
        assert!(settings_contents.contains("global_download_limit_bps = 9876"));
        assert!(host_contents.contains("client_id = \"host-override\""));
    }

    fn watch_env_guard() -> &'static std::sync::Mutex<()> {
        static GUARD: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        GUARD.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn test_configured_watch_paths_uses_local_watch_only_as_fallback() {
        let _guard = watch_env_guard().lock().unwrap();
        let original = env::var_os("SUPERSEEDR_WATCH_PATH_1");
        env::set_var("SUPERSEEDR_WATCH_PATH_1", "/bridge-watch");

        let explicit_watch = PathBuf::from("/shared-watch");
        let settings = Settings {
            watch_folder: Some(explicit_watch.clone()),
            ..Settings::default()
        };
        let configured = configured_watch_paths(&settings);
        let local_watch = get_watch_path().map(|(watch_path, _)| watch_path);

        assert!(configured.contains(&explicit_watch));
        assert!(configured.contains(&PathBuf::from("/bridge-watch")));
        if let Some(local_watch) = local_watch {
            assert!(!configured.contains(&local_watch));
        }

        if let Some(value) = original.clone() {
            env::set_var("SUPERSEEDR_WATCH_PATH_1", value);
        } else {
            env::remove_var("SUPERSEEDR_WATCH_PATH_1");
        }

        let fallback_paths = configured_watch_paths(&Settings::default());
        if let Some(local_watch) = get_watch_path().map(|(watch_path, _)| watch_path) {
            assert!(fallback_paths.contains(&local_watch));
        }

        if let Some(value) = original {
            env::set_var("SUPERSEEDR_WATCH_PATH_1", value);
        } else {
            env::remove_var("SUPERSEEDR_WATCH_PATH_1");
        }
    }

    #[test]
    fn test_resolve_additional_watch_paths_from_sources_orders_and_deduplicates() {
        let paths = resolve_additional_watch_paths_from_sources([
            ("SUPERSEEDR_WATCH_PATH_2", "/watch-b"),
            ("SUPERSEEDR_WATCH_PATH_10", "/watch-z"),
            ("IGNORED", "/nope"),
            ("SUPERSEEDR_WATCH_PATH_1", "/watch-a"),
            ("SUPERSEEDR_WATCH_PATH_3", "/watch-b"),
            ("SUPERSEEDR_WATCH_PATH_ALPHA", "/watch-alpha"),
            ("SUPERSEEDR_WATCH_PATH_4", ""),
        ]);

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/watch-a"),
                PathBuf::from("/watch-b"),
                PathBuf::from("/watch-z"),
                PathBuf::from("/watch-alpha"),
            ]
        );
    }
}
