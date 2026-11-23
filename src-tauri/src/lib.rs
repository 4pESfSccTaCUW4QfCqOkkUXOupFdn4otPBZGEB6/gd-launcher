//! Tauri command surface for the launcher.

mod config;
mod depot;
mod depot_session;
mod geode;
mod instance;
mod manifests;
mod mh;
mod mh_install;
mod state;
mod steam;
mod util;

use config::{Config, DepotBackend, Instance, Settings, VersionEntry};
use instance::InstanceView;
use serde::{Deserialize, Serialize};
use state::AppState;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
struct AppStateView {
    settings: Settings,
    instances: Vec<InstanceView>,
    versions: Vec<VersionEntry>,
    steam_installs: Vec<steam::SteamInstall>,
    data_dir: String,
    instances_dir: String,
    /// The real answer to "is Steam already signed in?" — read from
    /// DepotDownloader's own token store, not from a flag we set once.
    depot_session: depot_session::DepotSession,
}

fn views_for(cfg: &Config) -> Vec<InstanceView> {
    cfg.instances
        .iter()
        .map(|i| {
            let catalog = cfg.version(&i.manifest_id).map(|v| v.version.clone());
            instance::view(i, catalog)
        })
        .collect()
}

/// Paths the session reader should check beyond the .NET isolated store.
fn session_paths(app: &AppHandle) -> Vec<PathBuf> {
    depot::managed_depotdownloader(app)
        .parent()
        .map(|dir| vec![dir.join("account.config")])
        .unwrap_or_default()
}

fn current_session(app: &AppHandle) -> depot_session::DepotSession {
    depot_session::cached_session(&session_paths(app)).unwrap_or_default()
}

// ------------------------------------------------------------------- app state

#[tauri::command]
fn get_app_state(app: AppHandle, state: State<'_, AppState>) -> Result<AppStateView, String> {
    let cfg = state.snapshot()?;
    let data_dir = config::data_root(&app);
    let instances_dir = config::instances_root(&app, &cfg.settings);
    Ok(AppStateView {
        settings: cfg.settings.clone(),
        instances: views_for(&cfg),
        versions: cfg.versions.clone(),
        steam_installs: steam::find_installs(),
        data_dir: data_dir.display().to_string(),
        instances_dir: instances_dir.display().to_string(),
        depot_session: current_session(&app),
    })
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, mut settings: Settings) -> Result<(), String> {
    if !settings.remember_username {
        settings.steam_username.clear();
    }
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.settings = settings;
    }
    state.persist()
}

// ------------------------------------------------------------------ depot data

#[tauri::command]
async fn fetch_depot_info() -> Result<steam::DepotInfo, String> {
    steam::fetch_depot_info().await
}

#[tauri::command]
async fn current_manifest() -> Result<Option<serde_json::Value>, String> {
    Ok(steam::current_windows_manifest()
        .await
        .map(|(manifest, build)| serde_json::json!({ "manifestId": manifest, "buildId": build })))
}

#[tauri::command]
fn find_steam_installs() -> Vec<steam::SteamInstall> {
    steam::find_installs()
}

/// The account the depot pull will authenticate as, read straight from the local
/// Steam client. Passwords are not on disk — Valve keeps a refresh token — so this
/// is the username only.
#[tauri::command]
fn steam_account() -> Option<steam::SteamAccount> {
    steam::preferred_account()
}

/// Fall back to the detected Steam account when the caller did not name one, then
/// to the account the cached login token belongs to — a machine with no Steam
/// client but a stored token still pulls fine.
fn resolve_login(app: &AppHandle, username: String, use_qr: bool) -> String {
    if use_qr || !username.trim().is_empty() {
        return username;
    }
    if let Some(account) = steam::preferred_account() {
        return account.account_name;
    }
    current_session(app).username
}

#[tauri::command]
fn toolchain_status(app: AppHandle, state: State<'_, AppState>) -> Result<depot::ToolchainStatus, String> {
    depot::toolchain_status(&app, &state)
}

#[tauri::command]
async fn install_toolchain(app: AppHandle) -> Result<String, String> {
    depot::install_depotdownloader(app).await
}

// -------------------------------------------------------------- version catalogue

#[tauri::command]
fn add_version(state: State<'_, AppState>, entry: VersionEntry) -> Result<Vec<VersionEntry>, String> {
    if entry.manifest_id.trim().is_empty() {
        return Err("a manifest id is required".into());
    }
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        if cfg.versions.iter().any(|v| v.manifest_id == entry.manifest_id) {
            return Err("that manifest is already in the catalogue".into());
        }
        cfg.versions.push(VersionEntry {
            id: if entry.id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                entry.id
            },
            version: entry.version.trim().to_string(),
            build_id: entry.build_id.trim().to_string(),
            manifest_id: entry.manifest_id.trim().to_string(),
            depot_id: if entry.depot_id.trim().is_empty() {
                config::GD_WIN_DEPOT.to_string()
            } else {
                entry.depot_id.trim().to_string()
            },
            notes: entry.notes,
            added_at: config::now_secs(),
        });
        cfg.versions
            .sort_by(|a, b| b.added_at.cmp(&a.added_at));
    }
    state.persist()?;
    Ok(state.snapshot()?.versions)
}

#[tauri::command]
fn remove_version(state: State<'_, AppState>, id: String) -> Result<Vec<VersionEntry>, String> {
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.versions.retain(|v| v.id != id);
    }
    state.persist()?;
    Ok(state.snapshot()?.versions)
}

#[derive(Serialize)]
struct KnownVersion {
    version: String,
    date: String,
    manifest_id: String,
    depot_id: String,
    source: String,
    note: String,
    /// Recommended Mega Hack major for this build ("v5" .. "v9").
    mega_hack: String,
}

/// One table for the picker: live public manifest, bundled history, plus anything
/// the user has added by hand.
#[tauri::command]
async fn known_versions(state: State<'_, AppState>) -> Result<Vec<KnownVersion>, String> {
    let cfg = state.snapshot()?;
    let mut out: Vec<KnownVersion> = Vec::new();

    if let Some((manifest, _build)) = steam::current_windows_manifest().await {
        out.push(KnownVersion {
            version: "latest".into(),
            date: "9999-12-31".into(),
            manifest_id: manifest,
            depot_id: manifests::GD_DEPOT.into(),
            source: "live".into(),
            note: "current public branch".into(),
            mega_hack: mh::major_label("2.2081"),
        });
    }

    for b in manifests::BUNDLED {
        out.push(KnownVersion {
            version: b.version.into(),
            date: b.date.into(),
            manifest_id: b.manifest.into(),
            depot_id: manifests::GD_DEPOT.into(),
            source: "bundled".into(),
            note: String::new(),
            mega_hack: mh::major_label(b.version),
        });
    }

    for v in &cfg.versions {
        if out.iter().any(|k| k.manifest_id == v.manifest_id) {
            continue;
        }
        out.push(KnownVersion {
            version: v.version.clone(),
            date: manifests::civil_date(v.added_at),
            manifest_id: v.manifest_id.clone(),
            depot_id: v.depot_id.clone(),
            source: "added".into(),
            note: v.notes.clone(),
            mega_hack: mh::major_label(&v.version),
        });
    }

    // Live first, then the bundled history newest-first, then hand-added rows.
    let rank = |s: &str| match s {
        "live" => 0,
        "bundled" => 1,
        _ => 2,
    };
    out.sort_by(|a, b| {
        rank(&a.source)
            .cmp(&rank(&b.source))
            .then_with(|| b.date.cmp(&a.date))
    });
    Ok(out)
}

#[derive(Deserialize)]
struct QuickInstallRequest {
    version: String,
    manifest_id: String,
    depot_id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    use_qr: bool,
    start_download: bool,
}

/// One click from the version table: make the instance and start the pull.
#[tauri::command]
fn quick_install(
    app: AppHandle,
    state: State<'_, AppState>,
    request: QuickInstallRequest,
) -> Result<InstanceView, String> {
    let manifest = request.manifest_id.trim().to_string();
    if manifest.is_empty() {
        return Err("a manifest id is required".into());
    }
    let depot = if request.depot_id.trim().is_empty() {
        config::GD_WIN_DEPOT.to_string()
    } else {
        request.depot_id.trim().to_string()
    };
    let label = if request.version.trim().is_empty() {
        "custom".to_string()
    } else {
        request.version.trim().to_string()
    };

    let cfg = state.snapshot()?;
    let root = config::instances_root(&app, &cfg.settings);
    std::fs::create_dir_all(&root).map_err(|e| format!("create {root:?}: {e}"))?;
    let dir = instance::unique_dir(&root, "Geometry Dash", &label);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {dir:?}: {e}"))?;

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("Geometry Dash {label}"),
        version: label.clone(),
        build_id: String::new(),
        manifest_id: manifest.clone(),
        depot_id: depot.clone(),
        path: dir.display().to_string(),
        created_at: config::now_secs(),
        linked: false,
    };

    instance::write_marker(&dir, &record)?;
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.instances.push(record.clone());
    }
    state.persist()?;

    if request.start_download {
        let login = resolve_login(&app, request.username, request.use_qr);
        depot::start_download(
            app,
            record.path.clone(),
            depot,
            manifest,
            login,
            request.password,
            request.use_qr,
        )?;
    }
    Ok(instance::view(&record, Some(label)))
}

// ------------------------------------------------------------------- instances

#[derive(Deserialize)]
struct CreateInstanceRequest {
    name: String,
    version: String,
    build_id: String,
    manifest_id: String,
    depot_id: String,
}

#[tauri::command]
fn create_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateInstanceRequest,
) -> Result<InstanceView, String> {
    let cfg = state.snapshot()?;
    let root = config::instances_root(&app, &cfg.settings);
    std::fs::create_dir_all(&root).map_err(|e| format!("create {root:?}: {e}"))?;

    let dir = instance::unique_dir(&root, &request.name, &request.version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {dir:?}: {e}"))?;

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name: if request.name.trim().is_empty() {
            format!("Geometry Dash {}", request.version)
        } else {
            request.name.trim().to_string()
        },
        version: request.version.trim().to_string(),
        build_id: request.build_id.trim().to_string(),
        manifest_id: request.manifest_id.trim().to_string(),
        depot_id: if request.depot_id.trim().is_empty() {
            config::GD_WIN_DEPOT.to_string()
        } else {
            request.depot_id.trim().to_string()
        },
        path: dir.display().to_string(),
        created_at: config::now_secs(),
        linked: false,
    };

    instance::write_marker(&dir, &record)?;
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.instances.push(record.clone());
    }
    state.persist()?;
    Ok(instance::view(&record, None))
}

#[derive(Deserialize)]
struct ImportRequest {
    /// Index into the steam install list as last reported to the UI.
    index: usize,
    /// "link" points at the Steam folder, "copy" duplicates it.
    mode: String,
    name: String,
}

#[tauri::command]
fn import_steam_install(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ImportRequest,
) -> Result<InstanceView, String> {
    let installs = steam::find_installs();
    let install = installs
        .get(request.index)
        .ok_or("that Steam install is no longer on disk")?;
    let source = PathBuf::from(&install.path);
    if !steam::looks_like_gd(&source) {
        return Err(format!(
            "{} has no GeometryDash.exe — that library entry is stale",
            source.display()
        ));
    }

    let cfg = state.snapshot()?;
    let root = config::instances_root(&app, &cfg.settings);
    let label = if request.name.trim().is_empty() {
        "Steam copy".to_string()
    } else {
        request.name.trim().to_string()
    };

    let (path, linked) = if request.mode == "link" {
        (source.clone(), true)
    } else {
        let dir = instance::unique_dir(&root, &label, &install.build_id);
        instance::copy_tree(&app, &source, &dir)?;
        (dir, false)
    };

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name: label,
        version: cfg
            .version(&install.manifest_id)
            .map(|v| v.version.clone())
            .unwrap_or_else(|| format!("build {}", install.build_id)),
        build_id: install.build_id.clone(),
        manifest_id: install.manifest_id.clone(),
        depot_id: config::GD_WIN_DEPOT.to_string(),
        path: path.display().to_string(),
        created_at: config::now_secs(),
        linked,
    };

    if !linked {
        instance::write_marker(&path, &record)?;
    }
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.instances.push(record.clone());
    }
    state.persist()?;
    Ok(instance::view(&record, None))
}

#[tauri::command]
fn remove_instance(
    state: State<'_, AppState>,
    id: String,
    delete_files: bool,
) -> Result<(), String> {
    let target = {
        let cfg = state.snapshot()?;
        cfg.instance(&id).cloned().ok_or("unknown instance")?
    };
    if delete_files && !target.linked {
        instance::delete_folder(&PathBuf::from(&target.path))?;
    }
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.instances.retain(|i| i.id != id);
    }
    state.persist()
}

#[tauri::command]
fn launch_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&id).ok_or("unknown instance")?;
    instance::launch(instance)?;
    if cfg.settings.close_on_launch {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.close();
        }
    }
    Ok(())
}

#[tauri::command]
fn instance_detail(state: State<'_, AppState>, id: String) -> Result<InstanceDetail, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&id).ok_or("unknown instance")?;
    let path = PathBuf::from(&instance.path);
    let catalog = cfg.version(&instance.manifest_id).map(|v| v.version.clone());
    Ok(InstanceDetail {
        view: instance::view(instance, catalog),
        mods: geode::list_installed(&path),
    })
}

#[derive(Serialize)]
struct InstanceDetail {
    view: InstanceView,
    mods: Vec<geode::InstalledMod>,
}

#[tauri::command]
fn open_instance_folder(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&id).ok_or("unknown instance")?;
    open_in_explorer(&PathBuf::from(&instance.path))
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    open_in_explorer(&PathBuf::from(path))
}

fn open_in_explorer(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map_err(|e| format!("explorer: {e}"))?;
    }
    Ok(())
}

// ------------------------------------------------------------------- downloads

#[derive(Deserialize)]
struct StartDownloadRequest {
    instance_id: String,
    manifest_id: String,
    depot_id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    use_qr: bool,
}

#[tauri::command]
fn start_depot_download(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartDownloadRequest,
) -> Result<(), String> {
    let cfg = state.snapshot()?;
    let instance = cfg
        .instance(&request.instance_id)
        .cloned()
        .ok_or("unknown instance")?;
    if instance.linked {
        return Err("this instance points at a live Steam folder — copy it first".into());
    }
    if request.manifest_id.trim().is_empty() {
        return Err("a manifest id is required to pull a specific build".into());
    }
    drop(cfg);
    let login = resolve_login(&app, request.username, request.use_qr);
    depot::start_download(
        app,
        instance.path,
        request.depot_id,
        request.manifest_id,
        login,
        request.password,
        request.use_qr,
    )
}

#[tauri::command]
fn send_depot_input(app: AppHandle, text: String) -> Result<(), String> {
    depot::send_input(&app, text)
}

#[tauri::command]
fn cancel_depot_download(app: AppHandle) -> Result<(), String> {
    depot::cancel(&app)
}

#[tauri::command]
fn download_running(app: AppHandle) -> bool {
    depot::is_running(&app)
}

// ------------------------------------------------------------------------ mods

#[tauri::command]
async fn search_mods(query: String, page: u64, per_page: u64) -> Result<geode::ModSearchPage, String> {
    geode::search_mods(query, page, per_page).await
}

#[tauri::command]
async fn install_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    mod_id: String,
) -> Result<geode::InstalledMod, String> {
    let path = {
        let cfg = state.snapshot()?;
        let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
        PathBuf::from(&instance.path)
    };
    if !geode::loader_present(&path) {
        return Err("Geode is not installed in this instance — install the loader first".into());
    }
    let card = geode::mod_detail(&mod_id).await?;
    geode::install_mod(&app, &path, &card).await?;
    let mods = geode::list_installed(&path);
    mods.into_iter()
        .find(|m| m.id == card.id)
        .ok_or_else(|| "mod installed but the package could not be read back".to_string())
}

#[tauri::command]
fn uninstall_mod(
    state: State<'_, AppState>,
    instance_id: String,
    mod_id: String,
) -> Result<Vec<geode::InstalledMod>, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    let path = PathBuf::from(&instance.path);
    geode::uninstall_mod(&path, &mod_id)?;
    Ok(geode::list_installed(&path))
}

#[tauri::command]
fn list_installed_mods(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<geode::InstalledMod>, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    Ok(geode::list_installed(&PathBuf::from(&instance.path)))
}

#[tauri::command]
async fn loader_versions() -> Result<Vec<geode::LoaderVersion>, String> {
    geode::loader_versions().await
}

// ------------------------------------------------------------------ mega hack

/// Which Mega Hack major a game build needs. `None` when the label is not a
/// recognisable Geometry Dash version.
#[tauri::command]
fn mega_hack_target(version: String) -> Option<mh::MegaHackTarget> {
    mh::target_for(&version)
}

/// Run a Mega Hack installer the user downloaded, with the instance as the working
/// directory so its folder picker starts in the right place.
#[tauri::command]
fn run_installer(
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
) -> Result<(), String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    let installer = PathBuf::from(&path);
    if !installer.exists() {
        return Err(format!("{path} no longer exists"));
    }
    let is_exe = installer
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if !is_exe {
        return Err("expected a Windows installer (.exe)".into());
    }
    let mut cmd = std::process::Command::new(&installer);
    cmd.current_dir(&instance.path);
    cmd.spawn()
        .map_err(|e| format!("spawn {}: {e}", installer.display()))?;
    Ok(())
}

#[tauri::command]
fn reset_steam_session(app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    // Delete the cached login token itself — a flag would only hide it and leave a
    // dead key on disk claiming a session that is not there.
    let removed = depot_session::forget(&session_paths(&app));
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.settings.steam_qr_done = false;
    }
    state.persist()?;
    Ok(removed)
}

// --------------------------------------------------------------- mega hack api

/// The catalogue bundle that would be installed for this instance, if any.
#[tauri::command]
async fn mega_hack_available(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Option<mh_install::Bundle>, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    let bundles = mh_install::catalog(&app).await?;
    // A Geode package is useless without a loader, so only ask for one when the
    // instance actually has Geode on disk.
    let want_geode =
        geode::loader_present(Path::new(&instance.path)) && !mh::below_2_2(&instance.version);
    Ok(mh_install::pick(&bundles, &instance.version, want_geode).cloned())
}

/// Download and install Mega Hack into an instance. No installer window, no folder
/// picker — the archive lands where the game looks.
#[tauri::command]
async fn mega_hack_install(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    release: String,
) -> Result<mh_install::InstallReport, String> {
    let (path, version, has_geode) = {
        let cfg = state.snapshot()?;
        let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
        let path = PathBuf::from(&instance.path);
        (
            path.clone(),
            instance.version.clone(),
            geode::loader_present(&path),
        )
    };
    let want_geode = has_geode && !mh::below_2_2(&version);
    mh_install::install(&app, &path, &version, &release, want_geode).await
}

#[tauri::command]
fn mega_hack_uninstall(state: State<'_, AppState>, instance_id: String) -> Result<usize, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    mh_install::uninstall(Path::new(&instance.path))
}

#[tauri::command]
fn mega_hack_license() -> mh_install::LicenseStatus {
    mh_install::license_status()
}

#[tauri::command]
fn mega_hack_installed(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Option<mh_install::Installed>, String> {
    let cfg = state.snapshot()?;
    let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
    Ok(mh_install::installed(Path::new(&instance.path)))
}

/// Open a link in the default browser. Kept in Rust so the webview never needs a
/// navigation capability.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("only http(s) links can be opened".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", trimmed])
            .spawn()
            .map_err(|e| format!("open browser: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn install_loader(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    version: Option<String>,
) -> Result<String, String> {
    let path = {
        let cfg = state.snapshot()?;
        let instance = cfg.instance(&instance_id).ok_or("unknown instance")?;
        // Geode ships for 2.2 and up only. Refusing here keeps a stale UI or a
        // hand-edited config from writing a loader into a 2.1 install.
        if mh::below_2_2(&instance.version) {
            return Err(format!(
                "Geode requires Geometry Dash 2.2 or newer — {} uses Mega Hack",
                instance.version
            ));
        }
        PathBuf::from(&instance.path)
    };
    geode::install_loader(&app, &path, version).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = config::data_root(app.handle());
            std::fs::create_dir_all(&dir).ok();
            app.manage(AppState::new(dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            save_settings,
            fetch_depot_info,
            current_manifest,
            find_steam_installs,
            steam_account,
            toolchain_status,
            install_toolchain,
            add_version,
            remove_version,
            known_versions,
            quick_install,
            create_instance,
            import_steam_install,
            remove_instance,
            launch_instance,
            instance_detail,
            open_instance_folder,
            open_path,
            start_depot_download,
            send_depot_input,
            cancel_depot_download,
            download_running,
            search_mods,
            install_mod,
            uninstall_mod,
            list_installed_mods,
            loader_versions,
            install_loader,
            mega_hack_target,
            run_installer,
            reset_steam_session,
            open_url,
            mega_hack_available,
            mega_hack_install,
            mega_hack_uninstall,
            mega_hack_license,
            mega_hack_installed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Kept for the unused-import lint when the depot constants are only referenced
/// in docs.
#[allow(dead_code)]
fn _depot_consts() -> (&'static str, &'static str, DepotBackend) {
    (config::GD_MAC_DEPOT, config::GD_SHARED_DEPOT, DepotBackend::DepotDownloader)
}
