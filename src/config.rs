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

/// Folder name under `%APPDATA%` that holds every file this tool owns. It has
/// been this string since the first public build, so the data folder does not
/// move when the app does.
pub const APP_ID: &str = "org.gdlauncher.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepotBackend {
    /// SteamRE/DepotDownloader - self-contained, auto-fetched on first use.
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
    /// Bare `gd` opens the full screen interface instead of printing help.
    pub tui: bool,
    /// Palette name for the full screen interface. Unknown names fall back to
    /// the default, so a hand-edited config cannot leave the screen unreadable.
    pub tui_theme: String,
    /// Ask before a remove that deletes files.
    pub tui_confirm_remove: bool,
    /// Extra arguments appended to `GeometryDash.exe` on every launch.
    pub game_args: String,
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
            tui: false,
            tui_theme: "amber".into(),
            tui_confirm_remove: true,
            game_args: String::new(),
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
        // block startup - fall back to defaults and let the user re-add instances.
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

/// Root for everything this tool owns.
pub fn data_root() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(base) => PathBuf::from(base).join(APP_ID),
        // No APPDATA means the caller is in a stripped environment; a relative
        // folder keeps the tool usable instead of panicking.
        None => PathBuf::from(".gd-launcher"),
    }
}

/// The app id changed once, before the first public release, which leaves the
/// pre-release data in a sibling folder. No folder name is hard-coded: a
/// sibling counts as a previous root only when its `config.json` parses as this
/// `Config` *and* lists an instance that lives inside it. That signature is
/// this app's own layout - nothing else on disk produces it - so the scan can
/// never grab another program's folder.
fn find_previous_root(parent: &Path, current: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.as_path() != current)
        .collect();
    candidates.sort();
    candidates.into_iter().find(|candidate| {
        let Ok(raw) = std::fs::read_to_string(candidate.join("config.json")) else {
            return false;
        };
        let Ok(cfg) = serde_json::from_str::<Config>(&raw) else {
            return false;
        };
        cfg.instances
            .iter()
            .any(|instance| Path::new(&instance.path).starts_with(candidate))
    })
}

/// Move a previous data folder to `dir` and fix up the absolute paths inside
/// it. Returns true when a migration actually happened.
pub fn migrate_legacy_root(dir: &Path) -> bool {
    if dir.exists() {
        return false;
    }
    let Some(parent) = dir.parent() else {
        return false;
    };
    let Some(old) = find_previous_root(parent, dir) else {
        return false;
    };
    // Same volume, so this is a rename, not a copy - 3 GB of instances does not
    // get duplicated.
    if std::fs::rename(&old, dir).is_err() {
        return false;
    }

    let old_root = old.display().to_string();
    let new_root = dir.display().to_string();
    let path = dir.join("config.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Ok(mut cfg) = serde_json::from_str::<Config>(&raw) else {
        return true;
    };
    let mut touched = false;
    for instance in &mut cfg.instances {
        if let Some(rest) = instance.path.strip_prefix(&old_root) {
            instance.path = format!("{new_root}{rest}");
            touched = true;
        }
    }
    if let Some(rest) = cfg.settings.instances_dir.strip_prefix(&old_root) {
        cfg.settings.instances_dir = format!("{new_root}{rest}");
        touched = true;
    }
    if touched {
        let _ = cfg.save(dir);
    }
    true
}

/// Where managed instance folders live. Pure, so every caller resolves it the
/// same way.
pub fn instances_root_for(data_dir: &Path, settings: &Settings) -> PathBuf {
    if settings.instances_dir.trim().is_empty() {
        data_dir.join("instances")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh, empty scratch directory per test - the migration moves real
    /// folders around, so nothing may touch the live app data dir.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gd-launcher-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn instance_at(id: &str, path: &Path) -> Instance {
        Instance {
            id: id.into(),
            name: "2.2081".into(),
            version: "2.2081".into(),
            build_id: "1".into(),
            manifest_id: "2".into(),
            depot_id: GD_WIN_DEPOT.into(),
            path: path.display().to_string(),
            created_at: 0,
            linked: false,
        }
    }

    #[test]
    fn migration_moves_the_folder_and_rewrites_instance_paths() {
        let base = scratch("migrate");
        let old = base.join("previous-root");
        let new = base.join("org.gdlauncher.desktop");
        let old_instance = old.join("instances").join("steam-1-2");
        std::fs::create_dir_all(&old_instance).unwrap();
        std::fs::write(old_instance.join("GeometryDash.exe"), b"MZ").unwrap();

        let mut cfg = Config::default();
        cfg.instances.push(instance_at("steam-1-2", &old_instance));
        cfg.settings.instances_dir = old.join("instances").display().to_string();
        cfg.save(&old).unwrap();

        assert!(migrate_legacy_root(&new));
        assert!(new.exists(), "new root should exist after the rename");
        assert!(!old.exists(), "legacy root should be gone");

        let new_instance = new.join("instances").join("steam-1-2");
        assert!(new_instance.join("GeometryDash.exe").exists(), "files moved with the folder");

        let moved = Config::load(&new);
        assert_eq!(moved.instances.len(), 1);
        assert_eq!(moved.instances[0].path, new_instance.display().to_string());
        assert_eq!(
            moved.settings.instances_dir,
            new.join("instances").display().to_string()
        );
    }

    #[test]
    fn migration_does_nothing_without_a_legacy_folder() {
        let base = scratch("migrate-empty");
        let new = base.join("org.gdlauncher.desktop");
        assert!(!migrate_legacy_root(&new));
        assert!(!new.exists(), "nothing to move, nothing created");
    }

    #[test]
    fn migration_never_clobbers_an_existing_root() {
        let base = scratch("migrate-both");
        std::fs::create_dir_all(base.join("previous-root")).unwrap();
        let new = base.join("org.gdlauncher.desktop");
        std::fs::create_dir_all(&new).unwrap();

        assert!(!migrate_legacy_root(&new));
        assert!(
            base.join("previous-root").exists(),
            "previous folder left untouched"
        );
    }

    #[test]
    fn a_sibling_that_is_not_ours_is_left_alone() {
        let base = scratch("migrate-foreign");
        // Someone else's app data: a config file, but no instance pointing at itself.
        let foreign = base.join("some-other-app");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("config.json"), br#"{"theme":"dark"}"#).unwrap();

        let new = base.join("org.gdlauncher.desktop");
        assert!(!migrate_legacy_root(&new));
        assert!(foreign.join("config.json").exists());
    }
}
