//! Geode SDK integration: index search, mod install/remove, loader install.
//!
//! Mods are `.geode` files (plain zips with a `mod.json` at the root) dropped into
//! `<instance>/geode/mods`. The loader bundle is a flat zip unpacked over the
//! instance root - it carries Geode.dll plus the XInput1_4.dll proxy.

use crate::util;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const API: &str = "https://api.geode-sdk.org/v1";

#[derive(Debug, Clone, Serialize)]
pub struct ModCard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub downloads: u64,
    pub featured: bool,
    pub developers: Vec<String>,
    pub gd_win: String,
    pub geode: String,
    pub download_link: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModSearchPage {
    pub mods: Vec<ModCard>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub developer: String,
    pub description: String,
    pub gd_win: String,
    pub file: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoaderVersion {
    pub version: String,
    pub gd_win: String,
    pub prerelease: bool,
    pub url: String,
    pub hash: String,
    pub created_at: String,
}

// ---------------------------------------------------------------- index search

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    payload: Option<T>,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct SearchPayload {
    #[serde(default)]
    data: Vec<RawMod>,
    /// Total matches for the query - the index reports this as `count`, not `total`.
    #[serde(default)]
    count: u64,
}

#[derive(Debug, Deserialize)]
struct RawMod {
    id: String,
    #[serde(default)]
    featured: bool,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    developers: Vec<RawDeveloper>,
    #[serde(default)]
    versions: Vec<RawVersion>,
}

#[derive(Debug, Deserialize)]
struct RawDeveloper {
    #[serde(default)]
    username: String,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    download_link: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    geode: String,
    #[serde(default)]
    gd: GdTargets,
}

#[derive(Debug, Default, Deserialize)]
struct GdTargets {
    #[serde(default)]
    win: String,
}

fn card_from(raw: &RawMod) -> Option<ModCard> {
    // The index returns newest first; take the first accepted build.
    let v: &RawVersion = raw.versions.first()?;
    Some(ModCard {
        id: raw.id.clone(),
        name: if v.name.is_empty() { raw.id.clone() } else { v.name.clone() },
        description: v.description.clone(),
        version: v.version.clone(),
        downloads: raw.download_count,
        featured: raw.featured,
        developers: raw
            .developers
            .iter()
            .map(|d| d.display_name.clone().unwrap_or_else(|| d.username.clone()))
            .collect(),
        gd_win: v.gd.win.clone(),
        geode: v.geode.clone(),
        download_link: v.download_link.clone(),
        hash: v.hash.clone(),
    })
}

pub async fn search_mods(query: String, page: u64, per_page: u64) -> Result<ModSearchPage, String> {
    let client = util::http_client()?;
    let per_page = per_page.clamp(1, 50);
    let mut req = client
        .get(format!("{API}/mods"))
        .query(&[("page", page.to_string()), ("per_page", per_page.to_string())]);
    if !query.trim().is_empty() {
        req = req.query(&[("query", query.trim().to_string())]);
    }
    let resp = req.send().await.map_err(|e| format!("geode index: {e}"))?;
    let body: ApiEnvelope<SearchPayload> = resp
        .json()
        .await
        .map_err(|e| format!("geode index returned junk: {e}"))?;
    let payload = body
        .payload
        .ok_or_else(|| if body.error.is_empty() { "geode index: empty payload".to_string() } else { body.error })?;

    let mods: Vec<ModCard> = payload.data.iter().filter_map(card_from).collect();
    Ok(ModSearchPage {
        total: if payload.count == 0 {
            mods.len() as u64
        } else {
            payload.count
        },
        page,
        per_page,
        mods,
    })
}

/// Newest accepted build of one mod, straight from the detail endpoint.
pub async fn mod_detail(mod_id: &str) -> Result<ModCard, String> {
    let client = util::http_client()?;
    let resp = client
        .get(format!("{API}/mods/{mod_id}"))
        .send()
        .await
        .map_err(|e| format!("geode index: {e}"))?;
    let body: ApiEnvelope<RawMod> = resp
        .json()
        .await
        .map_err(|e| format!("geode index returned junk: {e}"))?;
    let raw = body
        .payload
        .ok_or_else(|| format!("mod {mod_id} not found"))?;
    card_from(&raw).ok_or_else(|| format!("mod {mod_id} has no published versions"))
}

// ------------------------------------------------------------------ mod files

fn mods_dir(instance_path: &Path) -> PathBuf {
    instance_path.join("geode").join("mods")
}

/// Download one mod build and drop it in the instance's mods folder.
pub async fn install_mod(instance_path: &Path, card: &ModCard) -> Result<String, String> {
    if !instance_path.exists() {
        return Err("instance folder is missing".into());
    }
    let dir = mods_dir(instance_path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {dir:?}: {e}"))?;

    let client = util::http_client()?;
    let resp = client
        .get(&card.download_link)
        .send()
        .await
        .map_err(|e| format!("download {}: {e}", card.id))?;
    if !resp.status().is_success() {
        return Err(format!("download {}: HTTP {}", card.id, resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("download {}: {e}", card.id))?;

    if !card.hash.is_empty() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest != card.hash.to_lowercase() {
            return Err(format!(
                "{} failed the index hash check - refusing to install a corrupted build",
                card.id
            ));
        }
    }

    let target = dir.join(format!("{}.geode", util::sanitize_component(&card.id)));
    std::fs::write(&target, &bytes).map_err(|e| format!("write {target:?}: {e}"))?;
    Ok(target.display().to_string())
}

pub fn list_installed(instance_path: &Path) -> Vec<InstalledMod> {
    let dir = mods_dir(instance_path);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_geode = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("geode"))
            .unwrap_or(false);
        if !is_geode || !path.is_file() {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let file = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default()
            .to_string();
        match read_mod_manifest(&path) {
            Some(m) => out.push(InstalledMod {
                id: m.id,
                name: m.name,
                version: m.version,
                developer: m.developers.join(", "),
                description: m.description,
                gd_win: m.gd.win,
                file,
                size,
            }),
            None => out.push(InstalledMod {
                id: file.trim_end_matches(".geode").to_string(),
                name: file.trim_end_matches(".geode").to_string(),
                version: "?".into(),
                developer: String::new(),
                description: "mod.json missing or unreadable".into(),
                gd_win: String::new(),
                file,
                size,
            }),
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[derive(Debug, Deserialize)]
struct ModManifest {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    developers: Vec<String>,
    #[serde(default)]
    gd: GdTargets,
}

fn read_mod_manifest(path: &Path) -> Option<ModManifest> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    // mod.json sits at the archive root for a valid .geode package.
    let mut entry = archive.by_name("mod.json").ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    serde_json::from_str::<ModManifest>(&buf).ok()
}

pub fn uninstall_mod(instance_path: &Path, mod_id: &str) -> Result<(), String> {
    let dir = mods_dir(instance_path);
    let safe = util::sanitize_component(mod_id);
    let mut removed = false;
    for candidate in [
        dir.join(format!("{safe}.geode")),
        dir.join(format!("{safe}.disabled")),
        dir.join(&safe),
    ] {
        if candidate.exists() {
            if candidate.is_dir() {
                std::fs::remove_dir_all(&candidate).map_err(|e| format!("remove {candidate:?}: {e}"))?;
            } else {
                std::fs::remove_file(&candidate).map_err(|e| format!("remove {candidate:?}: {e}"))?;
            }
            removed = true;
        }
    }
    if removed {
        Ok(())
    } else {
        Err(format!("{mod_id} is not installed in this instance"))
    }
}

// -------------------------------------------------------------- loader install

#[derive(Debug, Deserialize)]
struct RawLoaderVersion {
    #[serde(default)]
    version: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    gd: GdTargets,
    #[serde(default)]
    downloads: LoaderDownloads,
}

#[derive(Debug, Default, Deserialize)]
struct LoaderDownloads {
    #[serde(default)]
    win: Option<LoaderAsset>,
}

#[derive(Debug, Default, Deserialize)]
struct LoaderAsset {
    #[serde(default)]
    url: String,
    #[serde(default)]
    hash: String,
}

pub async fn loader_versions() -> Result<Vec<LoaderVersion>, String> {
    let client = util::http_client()?;
    let resp = client
        .get(format!("{API}/loader/versions"))
        .send()
        .await
        .map_err(|e| format!("geode loader list: {e}"))?;
    let body: ApiEnvelope<Vec<RawLoaderVersion>> = resp
        .json()
        .await
        .map_err(|e| format!("geode loader list returned junk: {e}"))?;
    let raw = body.payload.ok_or("geode loader list: empty payload")?;
    Ok(raw
        .into_iter()
        .filter_map(|v| {
            let win = v.downloads.win?;
            if win.url.is_empty() {
                return None;
            }
            Some(LoaderVersion {
                version: v.version,
                gd_win: v.gd.win,
                prerelease: v.prerelease,
                url: win.url,
                hash: win.hash,
                created_at: v.created_at,
            })
        })
        .collect())
}

/// Unpack a loader bundle over an instance root. `version = None` picks the
/// newest stable build from the index.
pub async fn install_loader(
    instance_path: &Path,
    version: Option<String>,
    cache: &Path,
    mut on_progress: impl FnMut(util::DownloadProgress),
) -> Result<String, String> {
    if !instance_path.exists() {
        return Err("instance folder is missing".into());
    }
    let all = loader_versions().await?;
    let picked = match version {
        Some(v) => all
            .iter()
            .find(|l| l.version == v)
            .cloned()
            .ok_or_else(|| format!("loader {v} is not published"))?,
        None => all
            .iter()
            .find(|l| !l.prerelease)
            .cloned()
            .or_else(|| all.first().cloned())
            .ok_or("geode index returned no Windows loader builds")?,
    };

    let zip_path = cache.join(format!("geode-loader-{}.zip", picked.version));
    if !zip_path.exists() {
        util::download("Geode loader", &picked.url, &zip_path, &mut on_progress).await?;
    }
    if !picked.hash.is_empty() {
        let digest = util::sha256_file(&zip_path)?;
        if digest != picked.hash.to_lowercase() {
            let _ = std::fs::remove_file(&zip_path);
            return Err("loader bundle failed its hash check - re-run the install".into());
        }
    }
    util::extract_zip(&zip_path, instance_path)?;

    // Geode refuses to start without steam_appid.txt on a non-Steam copy.
    let appid = instance_path.join("steam_appid.txt");
    if !appid.exists() {
        let _ = std::fs::write(&appid, crate::config::GD_APP_ID);
    }
    Ok(picked.version)
}

/// True when the folder already has a working loader.
pub fn loader_present(instance_path: &Path) -> bool {
    instance_path.join("Geode.dll").exists()
}
