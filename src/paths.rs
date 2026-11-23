//! Every path this tool owns, resolved in one place.
//!
//! The CLI and the full screen interface both call these, so there is no second
//! copy of "where does the token live" to drift out of sync.

use crate::config::{self, DepotBackend, Settings};
use crate::depot_session::{self, DepotSession};
use std::path::{Path, PathBuf};

/// `%APPDATA%\org.gdlauncher.desktop`, or a relative folder when the caller is
/// in a stripped environment where APPDATA is not set.
pub fn default_data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("APPDATA is not set - pass --data-dir")?;
    Ok(base.join(config::APP_ID))
}

pub fn tools_dir(dir: &Path) -> PathBuf {
    dir.join("tools")
}

pub fn managed_depotdownloader(dir: &Path) -> PathBuf {
    tools_dir(dir).join("depotdownloader").join("DepotDownloader.exe")
}

pub fn cache_dir(dir: &Path) -> PathBuf {
    dir.join("cache")
}

pub fn mh_cache(dir: &Path) -> PathBuf {
    cache_dir(dir).join("mh")
}

pub fn instances_root(dir: &Path, settings: &Settings) -> PathBuf {
    config::instances_root_for(dir, settings)
}

/// DepotDownloader keeps its token next to the tool, so that file is checked
/// alongside the .NET isolated store.
pub fn session_paths(dir: &Path) -> Vec<PathBuf> {
    vec![tools_dir(dir).join("depotdownloader").join("account.config")]
}

pub fn session(dir: &Path) -> DepotSession {
    depot_session::cached_session(&session_paths(dir)).unwrap_or_default()
}

/// Pick the depot backend and the executable that drives it. Does not download
/// anything: the caller decides whether a missing DepotDownloader is fetched.
pub fn resolve_backend(
    dir: &Path,
    settings: &Settings,
) -> Result<(DepotBackend, PathBuf), String> {
    match settings.depot_backend {
        DepotBackend::DepotDownloader => {
            let override_path = PathBuf::from(settings.depotdownloader_path.trim());
            if !settings.depotdownloader_path.trim().is_empty() && override_path.exists() {
                return Ok((DepotBackend::DepotDownloader, override_path));
            }
            let managed = managed_depotdownloader(dir);
            if managed.exists() {
                return Ok((DepotBackend::DepotDownloader, managed));
            }
            Err("DepotDownloader is not installed yet".into())
        }
        DepotBackend::SteamCmd => {
            let override_path = PathBuf::from(settings.steamcmd_path.trim());
            if !settings.steamcmd_path.trim().is_empty() && override_path.exists() {
                return Ok((DepotBackend::SteamCmd, override_path));
            }
            if let Some(found) = crate::depot::find_steamcmd() {
                return Ok((DepotBackend::SteamCmd, found));
            }
            Err("steamcmd.exe not found - set its path in the config".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_and_cache_hang_off_the_data_dir() {
        let dir = Path::new(r"C:\data");
        assert_eq!(
            managed_depotdownloader(dir),
            PathBuf::from(r"C:\data\tools\depotdownloader\DepotDownloader.exe")
        );
        assert_eq!(mh_cache(dir), PathBuf::from(r"C:\data\cache\mh"));
    }

    #[test]
    fn instances_follow_the_settings_override() {
        let mut settings = Settings::default();
        assert_eq!(
            instances_root(Path::new(r"C:\data"), &settings),
            PathBuf::from(r"C:\data\instances")
        );
        settings.instances_dir = r"D:\gd".into();
        assert_eq!(
            instances_root(Path::new(r"C:\data"), &settings),
            PathBuf::from(r"D:\gd")
        );
    }
}
