//! Mega Hack install, done natively.
//!
//! Absolute's installer is a Qt app that pulls everything from a public catalogue:
//!
//!   GET https://absolllute.com/api/mega_hack/v9/install.json
//!   GET https://absolllute.com/api/mega_hack/v9/files/{version}/{file}
//!
//! Each catalogue entry names a release, the game build it targets, whether it is
//! the Geode or the standalone build, and the SHA-256 of the archive. So the
//! launcher can do the whole job itself: resolve the entry for the instance, fetch
//! the archive, verify it, and drop it where the game looks — no folder picker, no
//! "Detect Geometry Dash" guessing at the Steam copy.
//!
//! Two shapes of payload:
//!
//!   - Geode builds are a single `.geode` package (a zip with `mod.json`) and go
//!     into `<instance>/geode/mods`.
//!   - Standalone builds are a flat zip of DLLs (`hackpro.dll`, `hackproldr.dll`,
//!     `discord_game_sdk.dll`, `XInput9_1_0.dll`) unpacked over the instance root.
//!
//! A marker file records exactly what was written so the launcher can remove it
//! again without touching anything else in the folder.
//!
//! Paid releases (v6 and up) need an Absolute account. The catalogue itself is
//! public, but the launcher refuses to install a paid build until a license is on
//! disk — the one the official installer writes when you sign in.

use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const CATALOG_URL: &str = "https://absolllute.com/api/mega_hack/v9/install.json";
const FILE_BASE: &str = "https://absolllute.com/api/mega_hack/v9/files";
const MARKER: &str = ".mh_install_data";

#[derive(Debug, Deserialize)]
struct Catalog {
    #[serde(default)]
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    bundles: Vec<Bundle>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bundle {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub platform: Vec<String>,
    #[serde(default)]
    pub gjver: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub geode: bool,
    #[serde(default)]
    pub notice: String,
    #[serde(default)]
    pub hash: String,
}

impl Bundle {
    pub fn paid(&self) -> bool {
        major_of(&self.version) >= 6
    }

    pub fn url(&self) -> String {
        format!("{FILE_BASE}/{}/{}", self.version, self.file)
    }
}

fn major_of(version: &str) -> u32 {
    version
        .trim_start_matches('v')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Where the official installer keeps the account token after a sign-in.
pub fn license_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("absolllute.megahack").join("license"))
}

/// The bearer token from the license file, if the machine has one.
pub fn license_token() -> Option<String> {
    let path = license_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .filter(|t| !t.is_empty())
}

#[derive(Debug, Clone, Serialize)]
pub struct LicenseStatus {
    pub signed_in: bool,
    /// Path shown to the user; empty when nothing is on disk.
    pub path: String,
    /// First and last four characters, never the whole token.
    pub masked: String,
}

pub fn license_status() -> LicenseStatus {
    let path = license_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    match license_token() {
        Some(token) => {
            let masked = if token.len() > 8 {
                format!("{}…{}", &token[..4], &token[token.len() - 4..])
            } else {
                "stored".into()
            };
            LicenseStatus {
                signed_in: true,
                path,
                masked,
            }
        }
        None => LicenseStatus {
            signed_in: false,
            path,
            masked: String::new(),
        },
    }
}

// ------------------------------------------------------------------- catalogue

/// Windows bundles from the live catalogue, newest first as served.
///
/// The catalogue is cached next to the other launcher downloads so a failed fetch
/// does not take the install button down with it.
pub async fn catalog(app: &AppHandle) -> Result<Vec<Bundle>, String> {
    let cache_dir = crate::config::data_root(app).join("cache").join("mh");
    let cache_file = cache_dir.join("install.json");

    let fetched = async {
        let client = util::http_client()?;
        let resp = client
            .get(CATALOG_URL)
            .send()
            .await
            .map_err(|e| format!("GET catalogue: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("GET catalogue: HTTP {}", resp.status()));
        }
        resp.text().await.map_err(|e| format!("read catalogue: {e}"))
    }
    .await;

    let body = match fetched {
        Ok(body) => {
            let _ = std::fs::create_dir_all(&cache_dir);
            let _ = std::fs::write(&cache_file, &body);
            body
        }
        Err(err) => match std::fs::read_to_string(&cache_file) {
            Ok(cached) => cached,
            Err(_) => return Err(err),
        },
    };

    let parsed: Catalog = serde_json::from_str(&body).map_err(|e| format!("parse catalogue: {e}"))?;
    let mut out = Vec::new();
    for package in parsed.packages {
        for bundle in package.bundles {
            if bundle.platform.iter().any(|p| p == "win") {
                out.push(bundle);
            }
        }
    }
    Ok(out)
}

/// The bundle the launcher would install for this instance.
///
/// Only an exact game build counts. The catalogue is the installer's own list, and
/// a build it does not list is one Mega Hack has no package for — handing a 2.2074
/// package to a 2.2 folder would be worse than saying no. Geode builds are preferred
/// when the instance has Geode; the standalone build is used when it does not.
pub fn pick<'a>(bundles: &'a [Bundle], game: &str, want_geode: bool) -> Option<&'a Bundle> {
    let same_game: Vec<&Bundle> = bundles.iter().filter(|b| b.gjver == game).collect();
    same_game
        .iter()
        .find(|b| b.geode == want_geode)
        .or_else(|| same_game.first())
        .copied()
}

// --------------------------------------------------------------------- install

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub bundle: Bundle,
    pub geode: bool,
    pub files: Vec<String>,
    pub bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Installed {
    pub release: String,
    pub bundle: String,
    pub geode: bool,
    pub files: Vec<String>,
}

/// Download and install a Mega Hack build into an instance.
pub async fn install(
    app: &AppHandle,
    instance_path: &Path,
    game: &str,
    release: &str,
    want_geode: bool,
) -> Result<InstallReport, String> {
    if !instance_path.exists() {
        return Err("instance folder is missing".into());
    }
    let bundles = catalog(app).await?;
    let bundle = pick(&bundles, game, want_geode).ok_or_else(|| {
        format!("Mega Hack publishes no build for Geometry Dash {game} — use the installer")
    })?;

    if bundle.paid() && license_token().is_none() {
        return Err(format!(
            "{} is a paid release — sign in once in Absolute's installer, then install from here",
            bundle.name
        ));
    }

    let cache_dir = crate::config::data_root(app).join("cache").join("mh");
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("mkdir cache: {e}"))?;
    let archive = cache_dir.join(format!("{}-{}", bundle.version, bundle.file));

    let bytes = util::download_to_file(
        app,
        "mh://progress",
        &format!("Mega Hack {}", bundle.version),
        &bundle.url(),
        &archive,
    )
    .await?;

    if !bundle.hash.is_empty() {
        let digest = util::sha256_file(&archive)?;
        if digest != bundle.hash.to_lowercase() {
            let _ = std::fs::remove_file(&archive);
            return Err("Mega Hack archive failed its hash check — try again".into());
        }
    }

    let mut files: Vec<String> = Vec::new();
    if bundle.geode {
        let mods = instance_path.join("geode").join("mods");
        std::fs::create_dir_all(&mods).map_err(|e| format!("mkdir {mods:?}: {e}"))?;
        let dest = mods.join(&bundle.file);
        std::fs::copy(&archive, &dest).map_err(|e| format!("write {dest:?}: {e}"))?;
        files.push(format!("geode/mods/{}", bundle.file));
    } else {
        // Flat DLL drop over the instance root.
        let names = zip_entry_names(&archive)?;
        util::extract_zip(&archive, instance_path)?;
        files.extend(names);
    }

    let marker = Installed {
        release: release.to_string(),
        bundle: bundle.version.clone(),
        geode: bundle.geode,
        files: files.clone(),
    };
    let marker_path = instance_path.join(MARKER);
    std::fs::write(
        &marker_path,
        serde_json::to_vec_pretty(&marker).map_err(|e| format!("encode marker: {e}"))?,
    )
    .map_err(|e| format!("write {marker_path:?}: {e}"))?;

    Ok(InstallReport {
        geode: bundle.geode,
        bundle: bundle.clone(),
        files,
        bytes,
    })
}

/// What the launcher put in this folder, if anything.
pub fn installed(instance_path: &Path) -> Option<Installed> {
    let raw = std::fs::read(instance_path.join(MARKER)).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Remove everything the marker lists. Files that are already gone are skipped.
pub fn uninstall(instance_path: &Path) -> Result<usize, String> {
    let Some(marker) = installed(instance_path) else {
        return Err("Mega Hack was not installed by this launcher".into());
    };
    let mut removed = 0usize;
    for rel in &marker.files {
        // Marker contents are ours, but never let one escape the instance root.
        let rel_path = Path::new(rel);
        if rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let target = instance_path.join(rel_path);
        if std::fs::remove_file(&target).is_ok() {
            removed += 1;
        }
    }
    let _ = std::fs::remove_file(instance_path.join(MARKER));
    Ok(removed)
}

fn zip_entry_names(zip_path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open {zip_path:?}: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip {zip_path:?}: {e}"))?;
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        if let Some(name) = entry.enclosed_name() {
            out.push(name.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(version: &str, game: &str, geode: bool) -> Bundle {
        Bundle {
            name: format!("{version} ({})", if geode { "Geode" } else { "Standalone" }),
            group: version.into(),
            version: version.into(),
            file: if geode {
                "absolllute.megahack.geode".into()
            } else {
                "download.zip".into()
            },
            platform: vec!["win".into()],
            gjver: game.into(),
            auth: "v9".into(),
            geode,
            notice: String::new(),
            hash: String::new(),
        }
    }

    #[test]
    fn pick_prefers_the_matching_shape() {
        let list = vec![
            bundle("v9.1.3", "2.2081", true),
            bundle("v9.1.0-beta.7", "2.2081", false),
        ];
        // Geode instance gets the Geode package.
        assert_eq!(pick(&list, "2.2081", true).unwrap().version, "v9.1.3");
        // No loader on the instance: the standalone build is the only one that works.
        assert_eq!(pick(&list, "2.2081", false).unwrap().version, "v9.1.0-beta.7");
    }

    #[test]
    fn pick_never_borrows_another_game_build() {
        let list = vec![
            bundle("v7.1.1-beta.4", "2.113", false),
            bundle("v8.0.3", "2.206", true),
        ];
        // The stable v7.1.1.3 is not in the catalogue, but 2.113 is.
        assert_eq!(pick(&list, "2.113", false).unwrap().version, "v7.1.1-beta.4");
        // 2.2 has no package at all; a 2.206 build must not be handed to it.
        assert!(pick(&list, "2.2", false).is_none());
    }

    #[test]
    fn paid_starts_at_v6() {
        assert!(!bundle("v5.4", "2.113", false).paid());
        assert!(bundle("v6.1.0", "2.113", false).paid());
        assert!(bundle("v9.1.3", "2.2081", true).paid());
    }

    #[test]
    fn url_matches_the_installer_layout() {
        let b = bundle("v9.1.3", "2.2081", true);
        assert_eq!(
            b.url(),
            "https://absolllute.com/api/mega_hack/v9/files/v9.1.3/absolllute.megahack.geode"
        );
    }

    #[test]
    fn parses_the_live_catalogue_shape() {
        // Trimmed straight from absolllute.com/api/mega_hack/v9/install.json.
        let raw = r#"{
          "installer": {"version": "v1.0.1", "url": "https://absolllute.com/download/mega-hack-installer"},
          "gjver": {"win": {"2.113": 1511220108, "2.2081": 1768959392}},
          "packages": [
            {"name": "Mega Hack v7", "bundles": [
              {"name": "v7.1.1 (Standalone)", "group": "v7.1.1-beta.4", "version": "v7.1.1-beta.4",
               "file": "download.zip", "platform": ["win"], "gjver": "2.113", "auth": "v7",
               "geode": false, "notice": "Click TAB to open on Windows!",
               "hash": "b8432d576e4cdff5cedfb0dc991af1a27be83c5130dc8d362b7dc56ae42f032e"}
            ]},
            {"name": "Mega Hack v9", "bundles": [
              {"name": "v9.1.3 (Geode)", "group": "v9.1.3", "version": "v9.1.3",
               "file": "absolllute.megahack.geode", "platform": ["win", "android"],
               "gjver": "2.2081", "auth": "v9", "geode": true, "notice": "",
               "hash": "9410357ca49a7ed4e7bdefe87cf5f911e03ad48fce8fc0a7e4d6a56016cd8e11"}
            ]}
          ]
        }"#;

        let parsed: Catalog = serde_json::from_str(raw).unwrap();
        let mut bundles: Vec<Bundle> = Vec::new();
        for package in parsed.packages {
            bundles.extend(package.bundles.into_iter().filter(|b| b.platform.iter().any(|p| p == "win")));
        }
        assert_eq!(bundles.len(), 2);
        let v7 = pick(&bundles, "2.113", false).unwrap();
        assert_eq!(v7.version, "v7.1.1-beta.4");
        assert!(!v7.geode && v7.paid());
        assert_eq!(
            v7.url(),
            "https://absolllute.com/api/mega_hack/v9/files/v7.1.1-beta.4/download.zip"
        );
        let v9 = pick(&bundles, "2.2081", true).unwrap();
        assert_eq!(v9.file, "absolllute.megahack.geode");
        assert!(v9.hash.starts_with("9410357c"));
    }
}
