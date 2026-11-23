//! Steam library discovery and live depot metadata.
//!
//! Two sources of truth:
//!   * the local Steam install (`libraryfolders.vdf`) for "import what I already own";
//!   * api.steamcmd.net for the manifest ids attached to each branch right now.

use crate::config::{GD_APP_ID, GD_WIN_DEPOT};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct SteamInstall {
    pub library: String,
    pub path: String,
    pub build_id: String,
    pub manifest_id: String,
    pub size_on_disk: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotBranch {
    pub branch: String,
    pub manifest_id: String,
    pub build_id: String,
    pub updated_at: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotInfo {
    pub app_id: String,
    pub name: String,
    pub depots: Vec<DepotRow>,
    pub branches: Vec<DepotBranch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotRow {
    pub depot_id: String,
    pub os: String,
    pub manifest_id: String,
    pub branch: String,
    pub max_size: u64,
}

/// `HKCU\Software\Valve\Steam\SteamPath`.
pub fn steam_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key in ["Software\\Valve\\Steam", "Software\\WOW6432Node\\Valve\\Steam"] {
        if let Ok(k) = hkcu.open_subkey(key) {
            if let Ok(path) = k.get_value::<String, _>("SteamPath") {
                let p = PathBuf::from(path.replace('/', "\\"));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    // Registry miss (portable install) - try the two default locations.
    for candidate in [
        r"C:\Program Files (x86)\Steam",
        r"C:\Program Files\Steam",
    ] {
        let p = PathBuf::from(candidate);
        if p.join("steamapps").exists() {
            return Some(p);
        }
    }
    None
}

/// Every `steamapps` folder known to this machine.
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(root) = steam_root() else {
        return out;
    };
    let primary = root.join("steamapps");
    if primary.exists() {
        out.push(primary.clone());
    }
    let vdf = primary.join("libraryfolders.vdf");
    if let Ok(raw) = std::fs::read_to_string(&vdf) {
        for line in raw.lines() {
            let trimmed = line.trim();
            // libraryfolders.vdf changed shape between client versions; both
            // shapes keep the folder on a line that starts with a quoted path key.
            if let Some(value) = quoted_pair(trimmed) {
                let (key, val) = value;
                let looks_like_path = key == "path" || key.eq_ignore_ascii_case("path");
                if looks_like_path || val.contains(":\\") || val.contains(":/") {
                    let candidate = PathBuf::from(val.replace("\\\\", "\\").replace('/', "\\"));
                    let apps = candidate.join("steamapps");
                    if apps.exists() && !out.iter().any(|existing| same_path(existing, &apps)) {
                        out.push(apps);
                    }
                }
            }
        }
    }
    out
}

/// Pull `"key" "value"` out of a single VDF line.
fn quoted_pair(line: &str) -> Option<(String, String)> {
    let mut parts = line.split('"').skip(1).step_by(2);
    let key = parts.next()?.to_string();
    let val = parts.next()?.to_string();
    Some((key, val))
}

/// A line that is nothing but one quoted token - how `loginusers.vdf` opens an
/// account block (`"76561199402092901"`). `quoted_pair` needs two, so those
/// headers are read here instead.
fn quoted_key(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix('"')?;
    let end = rest.find('"')?;
    let key = &rest[..end];
    // Nothing but whitespace may follow the closing quote.
    rest[end + 1..].trim().is_empty().then_some(key)
}

/// Windows paths compare case-insensitively; two library entries for one folder
/// would otherwise show up as two installs.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

/// Every installed Geometry Dash copy, with the manifest the ACF recorded.
pub fn find_installs() -> Vec<SteamInstall> {
    let mut out = Vec::new();
    for apps in steam_libraries() {
        let acf = apps.join(format!("appmanifest_{GD_APP_ID}.acf"));
        let Ok(raw) = std::fs::read_to_string(&acf) else {
            continue;
        };
        let installdir = acf_value(&raw, "installdir").unwrap_or_else(|| "Geometry Dash".into());
        let path = apps.join("common").join(&installdir);
        if !path.exists() {
            continue;
        }
        let build_id = acf_value(&raw, "buildid").unwrap_or_default();
        let manifest_id = acf_value(&raw, "manifest").unwrap_or_default();
        let size_on_disk = acf_value(&raw, "SizeOnDisk")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        out.push(SteamInstall {
            library: apps.display().to_string(),
            path: path.display().to_string(),
            build_id,
            manifest_id,
            size_on_disk,
        });
    }
    out
}

/// First `"key"  "value"` in an ACF, skipping the nested depot blocks by taking
/// the first hit only (app-level keys always precede `InstalledDepots`).
fn acf_value(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&needle) {
            let rest = rest.trim();
            if let Some(v) = rest.strip_prefix('"') {
                if let Some(end) = v.find('"') {
                    return Some(v[..end].to_string());
                }
            }
        }
    }
    None
}

/// Live depot table for Geometry Dash.
pub async fn fetch_depot_info() -> Result<DepotInfo, String> {
    let url = format!("https://api.steamcmd.net/v1/info/{GD_APP_ID}");
    let client = crate::util::http_client()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("steamcmd.net unreachable: {e}"))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("steamcmd.net returned junk: {e}"))?;

    let app = body
        .get("data")
        .and_then(|d| d.get(GD_APP_ID))
        .ok_or_else(|| "steamcmd.net has no record for 322170".to_string())?;

    let name = app
        .get("common")
        .and_then(|c| c.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Geometry Dash")
        .to_string();

    // Branch -> buildid, so each depot row can carry the build it belongs to.
    let mut branch_builds: Vec<DepotBranch> = Vec::new();
    if let Some(branches) = app.get("branches").and_then(|b| b.as_object()) {
        for (branch, info) in branches {
            let build_id = info
                .get("buildid")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let updated_at = info
                .get("timeupdated")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let description = info
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            branch_builds.push(DepotBranch {
                branch: branch.clone(),
                manifest_id: String::new(),
                build_id,
                updated_at,
                description,
            });
        }
    }

    let mut depots: Vec<DepotRow> = Vec::new();
    if let Some(table) = app.get("depots").and_then(|d| d.as_object()) {
        for (depot_id, info) in table {
            if !depot_id.chars().all(|c| c.is_ascii_digit()) {
                continue; // "baselanguages" / "branches" metadata rows
            }
            let os = info
                .get("config")
                .and_then(|c| c.get("oslist"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let max_size = info
                .get("maxsize")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let Some(manifests) = info.get("manifests").and_then(|m| m.as_object()) else {
                continue;
            };
            for (branch, man) in manifests {
                let manifest_id = man
                    .get("gid")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if manifest_id.is_empty() {
                    continue;
                }
                if manifest_id.is_empty() {
                    continue;
                }
                depots.push(DepotRow {
                    depot_id: depot_id.clone(),
                    os: os.clone(),
                    manifest_id,
                    branch: branch.clone(),
                    max_size,
                });
                if let Some(b) = branch_builds.iter_mut().find(|b| &b.branch == branch) {
                    if b.manifest_id.is_empty() {
                        b.manifest_id = depots.last().map(|d| d.manifest_id.clone()).unwrap_or_default();
                    }
                }
            }
        }
    }

    depots.sort_by(|a, b| {
        a.depot_id
            .cmp(&b.depot_id)
            .then_with(|| a.branch.cmp(&b.branch))
    });
    branch_builds.sort_by(|a, b| a.branch.cmp(&b.branch));

    Ok(DepotInfo {
        app_id: GD_APP_ID.to_string(),
        name,
        depots,
        branches: branch_builds,
    })
}

/// Steam's public manifest for the Windows depot, if the endpoint answers.
pub async fn current_windows_manifest() -> Option<(String, String)> {
    let info = fetch_depot_info().await.ok()?;
    let row = info
        .depots
        .iter()
        .find(|d| d.depot_id == GD_WIN_DEPOT && d.branch == "public")?;
    Some((row.manifest_id.clone(), info.branches.iter()
        .find(|b| b.branch == "public")
        .map(|b| b.build_id.clone())
        .unwrap_or_default()))
}

/// True when the folder looks like a Geometry Dash install.
pub fn looks_like_gd(path: &Path) -> bool {
    path.join("GeometryDash.exe").exists()
}

// ------------------------------------------------------------- steam account

#[derive(Debug, Clone, Serialize, Default)]
pub struct SteamAccount {
    pub steam_id: String,
    pub account_name: String,
    pub persona: String,
    pub remember_password: bool,
    pub auto_login: bool,
    /// True when this is the account the running Steam client is signed into.
    pub active: bool,
}

/// Account id (low 32 bits) the Steam client currently has signed in.
fn active_account_id() -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Software\\Valve\\Steam\\ActiveProcess").ok()?;
    let user: u32 = key.get_value("ActiveUser").ok()?;
    (user != 0).then_some(user)
}

/// Everything `loginusers.vdf` knows. Passwords are not in there - Valve stopped
/// storing them; the client keeps a refresh token instead.
pub fn steam_accounts() -> Vec<SteamAccount> {
    let Some(root) = steam_root() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(root.join("config").join("loginusers.vdf")) else {
        return Vec::new();
    };

    let active_id = active_account_id().map(|id| (76_561_197_960_265_728u64 + id as u64).to_string());

    let mut out: Vec<SteamAccount> = Vec::new();
    let mut current: Option<SteamAccount> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if current.is_none() {
            if let Some(key) = quoted_key(trimmed) {
                if key.len() >= 17 && key.chars().all(|c| c.is_ascii_digit()) {
                    current = Some(SteamAccount {
                        steam_id: key.to_string(),
                        ..Default::default()
                    });
                    continue;
                }
            }
        }
        if let Some((key, value)) = quoted_pair(trimmed) {
            if let Some(account) = current.as_mut() {
                match key.as_str() {
                    "AccountName" => account.account_name = value,
                    "PersonaName" => account.persona = value,
                    "RememberPassword" => account.remember_password = value == "1",
                    "AutoLogin" => account.auto_login = value == "1",
                    _ => {}
                }
            }
        } else if trimmed == "}" {
            if let Some(mut account) = current.take() {
                if let Some(id) = &active_id {
                    account.active = &account.steam_id == id;
                }
                out.push(account);
            }
        }
    }

    // Active account first, then whoever the client remembers.
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| b.remember_password.cmp(&a.remember_password))
    });
    out
}

/// The account a depot pull should authenticate as.
pub fn preferred_account() -> Option<SteamAccount> {
    let accounts = steam_accounts();
    accounts
        .iter()
        .find(|a| a.active && !a.account_name.is_empty())
        .cloned()
        .or_else(|| {
            accounts
                .iter()
                .find(|a| a.remember_password && !a.account_name.is_empty())
                .cloned()
        })
        .or_else(|| accounts.into_iter().find(|a| !a.account_name.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loginusers_headers_are_lone_quoted_ids() {
        // The account block opens with the id on its own; quoted_pair needs two
        // tokens and would miss it, which used to hide every account.
        assert_eq!(quoted_key("\"76561199402092901\""), Some("76561199402092901"));
        assert_eq!(quoted_key("  \"76561199402092901\"  "), Some("76561199402092901"));
        // Ordinary key/value lines must not be read as a header.
        assert_eq!(quoted_key("\"AccountName\"\t\t\"someone\""), None);
        assert_eq!(quoted_key("}"), None);
    }

    #[test]
    fn one_library_listed_twice_is_one_install() {
        // libraryfolders.vdf repeats the primary library with different casing.
        assert!(same_path(
            Path::new(r"C:\Program Files (x86)\Steam\steamapps"),
            Path::new(r"c:\program files (x86)\steam\steamapps"),
        ));
        assert!(!same_path(
            Path::new(r"C:\Steam\steamapps"),
            Path::new(r"D:\Steam\steamapps"),
        ));
    }
}
