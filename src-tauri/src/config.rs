//! Persistence for settings, registered instances and the depot-manifest catalogue.
//!
//! Everything lives in a single JSON document under the app data dir. Writes go
//! through a temp file + rename so a crash mid-save cannot leave a truncated
//! config behind (a corrupted config would silently orphan every instance).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Geometry Dash on Steam.
pub const GD_APP_ID: &str = "322170";
/// Windows content depot for Geometry Dash.
pub const GD_WIN_DEPOT: &str = "322171";
/// macOS content depot, kept around because the depot info endpoint returns it.
pub const GD_MAC_DEPOT: &str = "322172";
/// Shared Steamworks redistributable depot.
pub const GD_SHARED_DEPOT: &str = "228989";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepotBackend {
    /// SteamRE/DepotDownloader — self-contained, auto-fetched on first use.
    DepotDownloader,
    /// Valve's steamcmd. Requires a separate install.
    SteamCmd,
}

impl Default for DepotBackend {
    fn default() -> Self {
        Self::DepotDownloader
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub depot_backend: DepotBackend,
    /// Explicit override for the DepotDownloader.exe path. Empty means "use the managed copy".
    pub depotdownloader_path: String,
    /// Explicit override for steamcmd.exe.
    pub steamcmd_path: String,
    /// Remembered Steam login. The password is never written to disk.
    pub steam_username: String,
    pub remember_username: bool,
    /// Root folder that holds every managed instance. Empty means "<app data>/instances".
    pub instances_dir: String,
    /// Concurrent chunk connections handed to DepotDownloader.
    pub max_downloads: u32,
    /// Delete the depot cache (.DepotDownloader) after a successful pull.
    pub purge_depot_cache: bool,
    /// Close the launcher window when the game starts.
    pub close_on_launch: bool,
    /// Set once a QR sign-in has completed. The login key is cached from then on,
    /// so the QR flow is not offered again.
    pub steam_qr_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            depot_backend: DepotBackend::default(),
            depotdownloader_path: String::new(),
            steamcmd_path: String::new(),
            steam_username: String::new(),
            remember_username: true,
            instances_dir: String::new(),
            max_downloads: 8,
            purge_depot_cache: true,
            close_on_launch: false,
            steam_qr_done: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: String,
    /// Human label for the game build, e.g. "2.2081".
    pub version: String,
    /// Steam build id when known.
    pub build_id: String,
    /// Depot manifest gid used to produce this install. Empty for imported Steam copies.
    pub manifest_id: String,
    pub depot_id: String,
    pub path: String,
    pub created_at: i64,
    /// True when the folder is a live Steam install we only point at, not one we own.
    pub linked: bool,
}

/// One row of the "add a version" catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    pub version: String,
    pub build_id: String,
    pub manifest_id: String,
    pub depot_id: String,
    pub notes: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub settings: Settings,
    pub instances: Vec<Instance>,
    pub versions: Vec<VersionEntry>,
}

impl Config {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.json");
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => return Self::default(),
        };
        // A bad document is worth keeping on disk for inspection but must not
        // block startup — fall back to defaults and let the user re-add instances.
        match serde_json::from_str::<Config>(&raw) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("config.json unreadable ({err}); starting from defaults");
                let _ = std::fs::rename(&path, path.with_extension("json.broken"));
                Self::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("config.json");
        let tmp = dir.join("config.json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &path)
    }

    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn version(&self, manifest_id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.manifest_id == manifest_id)
    }
}

/// Root for everything this app owns.
pub fn data_root(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from(".").join("gd-launcher-data"))
}

pub fn instances_root(app: &tauri::AppHandle, settings: &Settings) -> PathBuf {
    if settings.instances_dir.trim().is_empty() {
        data_root(app).join("instances")
    } else {
        PathBuf::from(settings.instances_dir.trim())
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Folder-safe slug for instance directory names.
pub fn slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "instance".into()
    } else {
        out
    }
}
