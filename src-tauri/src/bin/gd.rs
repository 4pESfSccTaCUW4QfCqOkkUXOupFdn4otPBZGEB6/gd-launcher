//! `gd` — GD Launcher in a terminal.
//!
//! Same data folder, same `config.json`, same depot manifests and the same
//! DepotDownloader invocation as the window. An instance created here shows up
//! there, and the other way round.
//!
//! ```text
//! gd list
//! gd install 2.2074
//! gd launch 2.2074
//! ```
//!
//! Everything prints to stdout; progress, QR codes and warnings go to stderr so
//! `gd list > file` stays clean.

use gd_launcher_lib::{
    config::{self, Config, DepotBackend, Instance, Settings},
    depot, depot_session, geode, instance, manifests, mh, mh_install, steam, util,
};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

/// Bundle identifier the window ships under. Tauri resolves `app_data_dir` to
/// `%APPDATA%\<identifier>` on Windows, so both front ends read one folder.
const APP_ID: &str = "org.gdlauncher.desktop";

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("gd: {err}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = take_flag(&mut args, "-v") | take_flag(&mut args, "--verbose");
    let data_override = take_value(&mut args, "--data-dir");

    let dir = match data_override {
        Some(path) => PathBuf::from(path),
        None => default_data_dir()?,
    };

    let Some(command) = args.first().cloned() else {
        print_help();
        return Ok(());
    };
    args.remove(0);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("start runtime: {e}"))?;

    runtime.block_on(dispatch(&dir, &command, &args, verbose))
}

async fn dispatch(
    dir: &Path,
    command: &str,
    args: &[String],
    verbose: bool,
) -> Result<(), String> {
    match command {
        "list" | "ls" => cmd_list(dir),
        "versions" | "lsv" => cmd_versions(),
        "install" | "add" => cmd_install(dir, args, verbose).await,
        "launch" | "run" => cmd_launch(dir, args),
        "remove" | "rm" => cmd_remove(dir, args),
        "path" | "where" => cmd_path(dir, args),
        "steam" => cmd_steam(dir),
        "import" => cmd_import(dir, args, verbose),
        "login" | "signin" => cmd_login(dir, args, verbose).await,
        "mh" | "megahack" => cmd_mega_hack(dir, args, verbose).await,
        "mods" | "mod" => cmd_mods(dir, args).await,
        "loader" | "geode" => cmd_loader(dir, args, verbose).await,
        "doctor" => cmd_doctor(dir),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command `{other}` — try `gd help`")),
    }
}

// --------------------------------------------------------------------- paths

fn default_data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("APPDATA is not set — pass --data-dir")?;
    Ok(base.join(APP_ID))
}

fn instances_root(dir: &Path, settings: &Settings) -> PathBuf {
    config::instances_root_for(dir, settings)
}

/// DepotDownloader keeps its token next to the tool, so that file is checked
/// alongside the .NET isolated store.
fn session_paths(dir: &Path) -> Vec<PathBuf> {
    vec![dir
        .join("tools")
        .join("depotdownloader")
        .join("account.config")]
}

fn session(dir: &Path) -> depot_session::DepotSession {
    depot_session::cached_session(&session_paths(dir)).unwrap_or_default()
}

fn managed_depotdownloader(dir: &Path) -> PathBuf {
    dir.join("tools")
        .join("depotdownloader")
        .join("DepotDownloader.exe")
}

fn cache_dir(dir: &Path) -> PathBuf {
    dir.join("cache")
}

fn mh_cache(dir: &Path) -> PathBuf {
    cache_dir(dir).join("mh")
}

// ------------------------------------------------------------- arg utilities

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(pos) = args.iter().position(|a| a == name) {
        args.remove(pos);
        true
    } else {
        false
    }
}

fn take_value(args: &mut Vec<String>, name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    if pos + 1 >= args.len() {
        args.remove(pos);
        return None;
    }
    args.remove(pos);
    Some(args.remove(pos))
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

// ------------------------------------------------------------------- printer

/// Shared stderr writer: one live progress line, QR art that clears it first,
/// and a tail of recent log lines kept for the failure message.
///
/// The live line is only drawn when stderr is a terminal — piping `gd install`
/// into a file must not fill it with carriage returns and escape codes.
struct Printer {
    verbose: bool,
    line_active: bool,
    tty: bool,
}

impl Printer {
    fn new(verbose: bool) -> Self {
        use std::io::IsTerminal;
        Self {
            verbose,
            line_active: false,
            tty: std::io::stderr().is_terminal(),
        }
    }

    fn clear(&mut self) {
        if self.line_active {
            eprint!("\r\x1b[2K");
            self.line_active = false;
            let _ = std::io::stderr().flush();
        }
    }

    fn progress(&mut self, percent: f64, detail: &str) {
        if !self.tty {
            return;
        }
        let extra = compact_detail(detail);
        if extra.is_empty() {
            eprint!("\r\x1b[2K  {:>5.1}%", percent);
        } else {
            eprint!("\r\x1b[2K  {:>5.1}%  {}", percent, extra);
        }
        let _ = std::io::stderr().flush();
        self.line_active = true;
    }

    fn qr(&mut self, lines: &[String]) {
        self.clear();
        for line in lines {
            eprintln!("{line}");
        }
        let _ = std::io::stderr().flush();
    }

    fn log(&mut self, stream: &str, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        if self.verbose || stream == "err" {
            self.clear();
            eprintln!("  {line}");
        }
    }

    fn note(&mut self, text: &str) {
        self.clear();
        eprintln!("  {text}");
    }

    fn finish(&mut self) {
        self.clear();
    }
}

/// DepotDownloader prints `  42.17% C:\path\to\instance` and steamcmd prints
/// `Update state (0x61) downloading, progress: 42.17 (1234 / 2900)`. The path is
/// noise; the byte counters are worth keeping.
fn compact_detail(detail: &str) -> String {
    let detail = detail.trim();
    if let Some(open) = detail.find('(') {
        if let Some(close) = detail[open..].find(')') {
            return detail[open + 1..open + close].trim().to_string();
        }
    }
    String::new()
}

fn pump_stream<R: std::io::Read>(
    stream: R,
    tag: &'static str,
    printer: Arc<Mutex<Printer>>,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(stream);
    let mut raw: Vec<u8> = Vec::new();
    let mut decoder = depot::LinePump::new();

    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        while matches!(raw.last(), Some(b'\n') | Some(b'\r')) {
            raw.pop();
        }
        for event in decoder.feed(&raw, tag) {
            match event {
                depot::PumpEvent::Qr(lines) => {
                    if let Ok(mut out) = printer.lock() {
                        out.qr(&lines);
                    }
                }
                depot::PumpEvent::Progress { percent, detail } => {
                    if let Ok(mut out) = printer.lock() {
                        out.progress(percent, &detail);
                    }
                }
                depot::PumpEvent::Log { stream, line } => {
                    if let Ok(mut buf) = tail.lock() {
                        if buf.len() >= 40 {
                            buf.pop_front();
                        }
                        buf.push_back(line.clone());
                    }
                    if let Ok(mut out) = printer.lock() {
                        out.log(stream, &line);
                    }
                }
                depot::PumpEvent::AuthFailed(line) => {
                    if let Ok(mut out) = printer.lock() {
                        out.note(&format!("steam rejected the cached key ({line}) — `gd login`"));
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ toolchain

fn resolve_backend(
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
            if let Some(found) = depot::find_steamcmd() {
                return Ok((DepotBackend::SteamCmd, found));
            }
            Err("steamcmd.exe not found — set its path in `gd doctor` output".into())
        }
    }
}

/// Fetch DepotDownloader into the managed tools folder on first use.
async fn fetch_depotdownloader(dir: &Path) -> Result<PathBuf, String> {
    let tools = dir.join("tools").join("depotdownloader");
    std::fs::create_dir_all(&tools).map_err(|e| format!("create {tools:?}: {e}"))?;
    let zip = tools.join("depotdownloader.zip");

    eprintln!("  no DepotDownloader yet — fetching the 32 MB toolchain");
    let tty = {
        use std::io::IsTerminal;
        std::io::stderr().is_terminal()
    };
    let mut last = 0.0f64;
    util::download("DepotDownloader", depot::DD_DOWNLOAD_URL, &zip, |progress| {
        if !tty {
            return;
        }
        // The callback is synchronous and may fire often; one line per percent.
        if progress.percent - last >= 1.0 || progress.percent >= 100.0 {
            last = progress.percent;
            eprint!(
                "\r\x1b[2K  DepotDownloader  {:>5.1}%  {} / {}",
                progress.percent,
                util::human_bytes(progress.received),
                if progress.total > 0 {
                    util::human_bytes(progress.total)
                } else {
                    "?".into()
                }
            );
            let _ = std::io::stderr().flush();
        }
    })
    .await?;
    if tty {
        eprintln!();
    }
    util::extract_zip(&zip, &tools)?;
    let _ = std::fs::remove_file(&zip);

    let exe = tools.join("DepotDownloader.exe");
    if !exe.exists() {
        return Err("archive unpacked but DepotDownloader.exe is missing".into());
    }
    Ok(exe)
}

// ---------------------------------------------------------------------- pull

struct Login {
    username: String,
    password: String,
    use_qr: bool,
}

impl Login {
    fn anonymous() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            use_qr: false,
        }
    }
}

/// One finished child run: exit code plus the last lines it printed.
struct PullRun {
    code: i32,
    tail: Vec<String>,
}

impl PullRun {
    fn ok(&self) -> bool {
        self.code == 0
    }

    /// DepotDownloader refuses the login outright when `-username` names an
    /// account whose token it cannot read. That happens after the data folder
    /// moves, because the token store is keyed by the tool's own path.
    fn stale_key(&self) -> bool {
        self.tail.iter().any(|line| {
            line.contains("requires a username and password or access token")
                || line.contains("InvalidPassword")
                || line.contains("LoggedInElsewhere")
        })
    }

    fn report(&self) {
        for line in &self.tail {
            eprintln!("  {line}");
        }
    }
}

/// Run one depot pull to completion, streaming progress to stderr.
///
/// A cached key that DepotDownloader cannot read is retried once as a QR login
/// instead of failing: the token store lives under the tool's own path, so it
/// does not survive the launcher data folder moving.
async fn pull(
    dir: &Path,
    settings: &Settings,
    target: &Path,
    depot_id: &str,
    manifest_id: &str,
    login: &Login,
    verbose: bool,
) -> Result<(), String> {
    let first = run_pull(dir, settings, target, depot_id, manifest_id, login, verbose).await?;
    if first.ok() {
        purge_cache(settings, target);
        return Ok(());
    }

    if !login.use_qr && !login.username.trim().is_empty() && first.stale_key() {
        eprintln!("  the cached steam key is not readable from this install — scanning a code once");
        let qr = Login {
            username: String::new(),
            password: String::new(),
            use_qr: true,
        };
        let retry = run_pull(dir, settings, target, depot_id, manifest_id, &qr, verbose).await?;
        if retry.ok() {
            purge_cache(settings, target);
            return Ok(());
        }
        retry.report();
        return Err(format!("DepotDownloader exited with {}", retry.code));
    }

    first.report();
    Err(format!("DepotDownloader exited with {}", first.code))
}

fn purge_cache(settings: &Settings, target: &Path) {
    if settings.purge_depot_cache {
        // A full pull leaves several GB of chunk cache behind; the instance is
        // complete once the manifest validates.
        let _ = std::fs::remove_dir_all(target.join(".DepotDownloader"));
    }
}

async fn run_pull(
    dir: &Path,
    settings: &Settings,
    target: &Path,
    depot_id: &str,
    manifest_id: &str,
    login: &Login,
    verbose: bool,
) -> Result<PullRun, String> {
    let (backend, exe) = match resolve_backend(dir, settings) {
        Ok(pair) => pair,
        Err(err) => {
            if settings.depot_backend != DepotBackend::DepotDownloader {
                return Err(err);
            }
            (DepotBackend::DepotDownloader, fetch_depotdownloader(dir).await?)
        }
    };

    std::fs::create_dir_all(target).map_err(|e| format!("create {target:?}: {e}"))?;

    let mut cmd = depot::build_command(
        backend,
        &exe,
        target,
        depot_id,
        manifest_id,
        &login.username,
        &login.password,
        login.use_qr,
        settings.max_downloads,
    );
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let printer = Arc::new(Mutex::new(Printer::new(verbose)));
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    let out_thread = stdout.map(|stream| {
        let printer = printer.clone();
        let tail = tail.clone();
        std::thread::spawn(move || pump_stream(stream, "out", printer, tail))
    });
    let err_thread = stderr.map(|stream| {
        let printer = printer.clone();
        let tail = tail.clone();
        std::thread::spawn(move || pump_stream(stream, "err", printer, tail))
    });

    let status = child.wait().map_err(|e| format!("wait {}: {e}", exe.display()))?;
    if let Some(handle) = out_thread {
        let _ = handle.join();
    }
    if let Some(handle) = err_thread {
        let _ = handle.join();
    }
    if let Ok(mut out) = printer.lock() {
        out.finish();
    }

    let recent = tail
        .lock()
        .map(|buf| buf.iter().rev().take(10).cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok(PullRun {
        code: status.code().unwrap_or(-1),
        tail: recent.into_iter().rev().collect(),
    })
}

// ------------------------------------------------------------------ commands

fn cmd_list(dir: &Path) -> Result<(), String> {
    let cfg = Config::load(dir);
    if cfg.instances.is_empty() {
        println!("no instances yet — `gd versions` then `gd install <version>`");
        return Ok(());
    }

    println!(
        "{:<10} {:<14} {:<9} {:>9} {:>5}  {}",
        "ID", "VERSION", "STATE", "SIZE", "MODS", "PATH"
    );
    for entry in &cfg.instances {
        let view = instance::view(entry, None);
        let state = if !view.exists {
            "missing"
        } else if !view.has_exe {
            "no exe"
        } else if view.geode {
            "geode"
        } else {
            "ready"
        };
        println!(
            "{:<10} {:<14} {:<9} {:>9} {:>5}  {}",
            short_id(&entry.id),
            truncate(&entry.version, 14),
            state,
            view.size_human,
            view.mod_count,
            entry.path
        );
    }
    Ok(())
}

fn cmd_versions() -> Result<(), String> {
    println!("{:<10} {:<12} {}", "VERSION", "RELEASED", "MANIFEST");
    for entry in manifests::BUNDLED {
        println!(
            "{:<10} {:<12} {}",
            entry.version, entry.date, entry.manifest
        );
    }
    Ok(())
}

struct InstallOpts {
    version: String,
    manifest: Option<String>,
    name: Option<String>,
    anonymous: bool,
    qr: bool,
}

fn parse_install(args: &[String]) -> Result<InstallOpts, String> {
    let mut rest: Vec<String> = args.to_vec();
    let manifest = take_value(&mut rest, "--manifest");
    let name = take_value(&mut rest, "--name");
    let anonymous = take_flag(&mut rest, "--anonymous") | take_flag(&mut rest, "--anon");
    let qr = take_flag(&mut rest, "--qr");

    let Some(version) = rest.into_iter().next() else {
        return Err("usage: gd install <version> [--name X] [--manifest ID] [--qr]".into());
    };
    Ok(InstallOpts {
        version,
        manifest,
        name,
        anonymous,
        qr,
    })
}

async fn cmd_install(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let opts = parse_install(args)?;

    let bundled = manifests::BUNDLED
        .iter()
        .find(|b| b.version == opts.version)
        .or_else(|| {
            manifests::BUNDLED
                .iter()
                .find(|b| b.version.eq_ignore_ascii_case(&opts.version))
        });

    let (version, manifest) = match (bundled, opts.manifest.clone()) {
        (Some(entry), Some(custom)) if !custom.trim().is_empty() => {
            (entry.version.to_string(), custom.trim().to_string())
        }
        (Some(entry), _) => (entry.version.to_string(), entry.manifest.to_string()),
        (None, Some(custom)) if !custom.trim().is_empty() => {
            (opts.version.clone(), custom.trim().to_string())
        }
        (None, _) => {
            return Err(format!(
                "no bundled build called {} — `gd versions` lists them, or pass --manifest",
                opts.version
            ))
        }
    };

    let mut cfg = Config::load(dir);
    let root = instances_root(dir, &cfg.settings);
    std::fs::create_dir_all(&root).map_err(|e| format!("create {root:?}: {e}"))?;

    let name = opts
        .name
        .clone()
        .unwrap_or_else(|| format!("Geometry Dash {version}"));
    let target = instance::unique_dir(&root, &name, &version);
    std::fs::create_dir_all(&target).map_err(|e| format!("create {target:?}: {e}"))?;

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        version: version.clone(),
        build_id: String::new(),
        manifest_id: manifest.clone(),
        depot_id: config::GD_WIN_DEPOT.to_string(),
        path: target.display().to_string(),
        created_at: config::now_secs(),
        linked: false,
    };
    instance::write_marker(&target, &record)?;
    cfg.instances.push(record.clone());
    cfg.save(dir).map_err(|e| format!("save config: {e}"))?;

    println!(
        "instance  {}  {}  {}",
        short_id(&record.id),
        version,
        record.path
    );

    let cached = session(dir);
    let login = if opts.anonymous {
        Login::anonymous()
    } else if opts.qr || !cached.signed_in {
        if !cached.signed_in && !opts.qr {
            eprintln!("  no steam session — scan the code below with the Steam app");
        }
        Login {
            username: String::new(),
            password: String::new(),
            use_qr: true,
        }
    } else {
        Login {
            username: cached.username.clone(),
            password: String::new(),
            use_qr: false,
        }
    };

    pull(
        dir,
        &cfg.settings,
        &target,
        &record.depot_id,
        &manifest,
        &login,
        verbose,
    )
    .await?;

    let size = util::dir_size(&target);
    println!(
        "done      {}  {}  {} files",
        short_id(&record.id),
        util::human_bytes(size),
        count_files(&target)
    );
    Ok(())
}

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .count()
}

fn cmd_launch(dir: &Path, args: &[String]) -> Result<(), String> {
    let cfg = Config::load(dir);
    let needle = args
        .first()
        .ok_or("usage: gd launch <instance>")?;
    let record = resolve(&cfg, needle)?;
    let path = Path::new(&record.path);
    if !path.join("GeometryDash.exe").exists() {
        return Err(format!("{} has no GeometryDash.exe", path.display()));
    }
    instance::launch(record)?;
    println!("launched  {}  {}", short_id(&record.id), record.version);
    Ok(())
}

fn cmd_remove(dir: &Path, args: &[String]) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let delete_files = take_flag(&mut rest, "--delete-files");
    let needle = rest
        .first()
        .ok_or("usage: gd remove <instance> [--delete-files]")?
        .clone();

    let mut cfg = Config::load(dir);
    let target = resolve(&cfg, &needle)?.clone();
    let index = cfg
        .instances
        .iter()
        .position(|i| i.id == target.id)
        .ok_or("instance vanished")?;
    cfg.instances.remove(index);
    cfg.save(dir).map_err(|e| format!("save config: {e}"))?;

    if delete_files {
        let freed = util::dir_size(Path::new(&target.path));
        instance::delete_folder(Path::new(&target.path))?;
        println!(
            "removed   {}  {} freed  {}",
            short_id(&target.id),
            util::human_bytes(freed),
            target.path
        );
    } else {
        println!(
            "removed   {}  files kept at {}",
            short_id(&target.id),
            target.path
        );
    }
    Ok(())
}

fn cmd_path(dir: &Path, args: &[String]) -> Result<(), String> {
    let cfg = Config::load(dir);
    let needle = args.first().ok_or("usage: gd path <instance>")?;
    let record = resolve(&cfg, needle)?;
    println!("{}", record.path);
    Ok(())
}

fn cmd_steam(dir: &Path) -> Result<(), String> {
    let installs = steam::find_installs();
    println!("steam installs");
    if installs.is_empty() {
        println!("  none found");
    } else {
        for (index, install) in installs.iter().enumerate() {
            println!(
                "  [{}]  build {:<10}  {:<9}  {}",
                index,
                install.build_id,
                util::human_bytes(install.size_on_disk),
                install.path
            );
        }
    }

    let accounts = steam::steam_accounts();
    println!("steam accounts");
    if accounts.is_empty() {
        println!("  none found");
    } else {
        for account in accounts {
            println!(
                "  {}  {}{}",
                account.account_name,
                account.persona,
                if account.active { "  (active)" } else { "" }
            );
        }
    }

    let cached = session(dir);
    println!("depot session");
    if cached.signed_in {
        println!(
            "  signed in as {}  ({} )",
            cached.username,
            if cached.path.is_empty() {
                "path unknown"
            } else {
                &cached.path
            }
        );
    } else {
        println!("  no cached login — `gd login` scans a QR once");
    }
    Ok(())
}

fn cmd_import(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let link = take_flag(&mut rest, "--link");
    let copy = take_flag(&mut rest, "--copy");
    let name = take_value(&mut rest, "--name");
    let index: usize = rest
        .first()
        .ok_or("usage: gd import <index> [--link|--copy] [--name X]")?
        .parse()
        .map_err(|_| "index must be a number from `gd steam`".to_string())?;

    let installs = steam::find_installs();
    let install = installs
        .get(index)
        .ok_or("that Steam install is no longer on disk")?;
    let source = PathBuf::from(&install.path);
    if !steam::looks_like_gd(&source) {
        return Err(format!(
            "{} has no GeometryDash.exe — that library entry is stale",
            source.display()
        ));
    }

    let mut cfg = Config::load(dir);
    let root = instances_root(dir, &cfg.settings);
    let label = name.unwrap_or_else(|| "Steam copy".to_string());

    // Copying is the safe default: Steam keeps updating its own folder.
    let (path, linked) = if link && !copy {
        (source.clone(), true)
    } else {
        let target = instance::unique_dir(&root, &label, &install.build_id);
        eprintln!("  copying {} — this takes a minute", source.display());
        let tty = {
            use std::io::IsTerminal;
            std::io::stderr().is_terminal()
        };
        instance::copy_tree_in(&source, &target, |progress| {
            if !tty {
                return;
            }
            let percent = progress
                .get("percent")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let label = progress
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            eprint!("\r\x1b[2K  {:>5.1}%  {}", percent, truncate(label, 50));
            let _ = std::io::stderr().flush();
        })?;
        eprintln!();
        (target, false)
    };

    let version = cfg
        .version(&install.manifest_id)
        .map(|v| v.version.clone())
        .unwrap_or_else(|| format!("build {}", install.build_id));

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name: label,
        version,
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
    cfg.instances.push(record.clone());
    cfg.save(dir).map_err(|e| format!("save config: {e}"))?;

    println!(
        "imported  {}  {}{}  {}",
        short_id(&record.id),
        record.version,
        if linked { "  (linked)" } else { "" },
        record.path
    );
    if verbose && linked {
        eprintln!("  linked instances follow Steam's own updates");
    }
    Ok(())
}

async fn cmd_login(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let force = take_flag(&mut rest, "--force");

    let cached = session(dir);
    if cached.signed_in && !force {
        println!("already signed in as {}", cached.username);
        println!("`gd login --force` re-scans a code anyway");
        return Ok(());
    }

    // DepotDownloader has no login-only mode: `-qr` is answered during a pull.
    // The oldest build is the smallest, so that is the throwaway.
    let smallest = manifests::BUNDLED
        .last()
        .ok_or("bundled manifest table is empty")?;
    let scratch = dir.join("tmp").join("login");

    eprintln!(
        "  signing in — this pulls Geometry Dash {} once (~40 MB), then deletes it",
        smallest.version
    );

    let cfg = Config::load(dir);
    let login = Login {
        username: String::new(),
        password: String::new(),
        use_qr: true,
    };
    pull(
        dir,
        &cfg.settings,
        &scratch,
        config::GD_WIN_DEPOT,
        smallest.manifest,
        &login,
        verbose,
    )
    .await?;

    let _ = std::fs::remove_dir_all(&scratch);
    let after = session(dir);
    if after.signed_in {
        println!("signed in  {}", after.username);
    } else {
        println!("pull finished but no token was written — check the Steam app");
    }
    Ok(())
}

async fn cmd_mega_hack(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let Some(needle) = rest.first().cloned() else {
        return Err("usage: gd mh <instance> [install [release]]".into());
    };
    rest.remove(0);

    let cfg = Config::load(dir);
    let record = resolve(&cfg, &needle)?.clone();
    let path = PathBuf::from(&record.path);
    let version = record.version.clone();
    let has_geode = geode::loader_present(&path);

    let installed = mh_install::installed(&path);
    let license = mh_install::license_status();

    let action = rest.first().cloned().unwrap_or_else(|| "list".into());
    if action != "install" {
        println!("instance  {}  {}", short_id(&record.id), version);
        println!(
            "geode     {}",
            if has_geode { "loader present" } else { "no loader" }
        );
        println!(
            "licence   {}",
            if license.signed_in {
                format!("token {} on disk", license.masked)
            } else {
                "no Absolute account — paid releases will not install".into()
            }
        );
        match &installed {
            Some(entry) => println!("installed {}  ({})", entry.release, entry.bundle),
            None => println!("installed none"),
        }
        println!();

        let options = mh::options_for(&version);
        if options.is_empty() {
            println!("no Mega Hack release targets {version}");
            return Ok(());
        }
        let recommended = mh::recommended(&version).map(|r| r.version);
        println!("{:<18} {:<10} {:<8} {}", "RELEASE", "TARGET", "CHANNEL", "COST");
        for release in options {
            let channel = match (release.geode, release.standalone) {
                (true, true) => "geode+both",
                (true, false) => "geode",
                (false, true) => "standalone",
                (false, false) => "?",
            };
            let mark = if Some(&release.version) == recommended.as_ref() {
                "  <- recommended"
            } else {
                ""
            };
            println!(
                "{:<18} {:<10} {:<8} {}{}",
                release.version,
                release.game,
                channel,
                if release.paid { "paid" } else { "free" },
                mark
            );
        }
        return Ok(());
    }

    let requested = rest.get(1).cloned();
    let release = match requested {
        Some(name) => name,
        None => mh::recommended(&version)
            .map(|r| r.version)
            .ok_or_else(|| format!("no Mega Hack release targets {version}"))?,
    };
    let want_geode = has_geode && !mh::below_2_2(&version);

    let mut printer = Printer::new(verbose);
    let report = mh_install::install_in(
        &mh_cache(dir),
        &path,
        &version,
        &release,
        want_geode,
        |progress| printer.progress(progress.percent, &progress.label),
    )
    .await;
    printer.finish();
    let report = report?;

    println!(
        "installed {}  {}  {} files  {}",
        report.bundle.version,
        report.bundle.name,
        report.files.len(),
        util::human_bytes(report.bytes)
    );
    for file in &report.files {
        println!("  {file}");
    }
    Ok(())
}

async fn cmd_mods(dir: &Path, args: &[String]) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let Some(needle) = rest.first().cloned() else {
        return Err("usage: gd mods <instance> [list|search|install|remove]".into());
    };
    rest.remove(0);

    let cfg = Config::load(dir);
    let record = resolve(&cfg, &needle)?.clone();
    let path = PathBuf::from(&record.path);
    if !path.exists() {
        return Err(format!("{} is missing on disk", path.display()));
    }

    let action = rest.first().cloned().unwrap_or_else(|| "list".into());
    rest.remove(0);

    match action.as_str() {
        "list" | "ls" => {
            let mods = geode::list_installed(&path);
            if mods.is_empty() {
                println!("no mods installed");
                return Ok(());
            }
            println!("{:<28} {:<14} {:<18} {}", "ID", "VERSION", "DEVELOPER", "NAME");
            for entry in mods {
                println!(
                    "{:<28} {:<14} {:<18} {}",
                    truncate(&entry.id, 28),
                    truncate(&entry.version, 14),
                    truncate(&entry.developer, 18),
                    entry.name
                );
            }
            Ok(())
        }
        "search" | "find" => {
            let query = rest.join(" ");
            if query.trim().is_empty() {
                return Err("usage: gd mods <instance> search <text>".into());
            }
            let page = geode::search_mods(query.clone(), 1, 20).await?;
            if page.mods.is_empty() {
                println!("nothing matches `{query}`");
                return Ok(());
            }
            println!("{:<28} {:<14} {:>9}  {}", "ID", "VERSION", "DOWNLOADS", "NAME");
            for card in page.mods {
                println!(
                    "{:<28} {:<14} {:>9}  {}",
                    truncate(&card.id, 28),
                    truncate(&card.version, 14),
                    card.downloads,
                    card.name
                );
            }
            Ok(())
        }
        "install" | "add" => {
            let mod_id = rest
                .first()
                .cloned()
                .ok_or("usage: gd mods <instance> install <mod-id>")?;
            if !geode::loader_present(&path) {
                return Err("Geode is not installed here — `gd loader <instance> install`".into());
            }
            let card = geode::mod_detail(&mod_id).await?;
            let written = geode::install_mod(&path, &card).await?;
            println!("installed {}  {}  {}", card.id, card.version, written);
            Ok(())
        }
        "remove" | "rm" | "uninstall" => {
            let mod_id = rest
                .first()
                .cloned()
                .ok_or("usage: gd mods <instance> remove <mod-id>")?;
            geode::uninstall_mod(&path, &mod_id)?;
            println!("removed   {mod_id}");
            Ok(())
        }
        other => Err(format!("unknown mods action `{other}`")),
    }
}

async fn cmd_loader(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let mut rest: Vec<String> = args.to_vec();
    let Some(needle) = rest.first().cloned() else {
        return Err("usage: gd loader <instance> [install [version]]".into());
    };
    rest.remove(0);

    let cfg = Config::load(dir);
    let record = resolve(&cfg, &needle)?.clone();
    let path = PathBuf::from(&record.path);
    if !path.exists() {
        return Err(format!("{} is missing on disk", path.display()));
    }

    let action = rest.first().cloned().unwrap_or_else(|| "status".into());
    if action == "status" || action == "list" {
        if geode::loader_present(&path) {
            println!("geode     loader present in {}", path.display());
            println!("mods      {}", geode::list_installed(&path).len());
        } else {
            println!("geode     not installed");
        }
        return Ok(());
    }
    if action != "install" && action != "add" {
        return Err(format!("unknown loader action `{action}`"));
    }

    let requested = rest.get(1).cloned();
    let mut printer = Printer::new(verbose);
    let version = geode::install_loader_in(&path, requested, &cache_dir(dir), |progress| {
        printer.progress(progress.percent, &progress.label)
    })
    .await;
    printer.finish();
    let version = version?;
    println!("installed geode {version}  {}", path.display());
    Ok(())
}

fn cmd_doctor(dir: &Path) -> Result<(), String> {
    let cfg = Config::load(dir);
    println!("data        {}", dir.display());
    println!(
        "instances   {}",
        instances_root(dir, &cfg.settings).display()
    );
    println!(
        "backend     {}",
        match cfg.settings.depot_backend {
            DepotBackend::DepotDownloader => "depotdownloader",
            DepotBackend::SteamCmd => "steamcmd",
        }
    );

    match resolve_backend(dir, &cfg.settings) {
        Ok((_, exe)) => println!("toolchain   {}", exe.display()),
        Err(err) => println!("toolchain   {err} — it is fetched on first install"),
    }

    let cached = session(dir);
    if cached.signed_in {
        println!("session     {} ({})", cached.username, cached.path);
    } else {
        println!("session     none — `gd login`");
    }

    let license = mh_install::license_status();
    if license.signed_in {
        println!("megahack    licence {} on disk", license.masked);
    } else {
        println!("megahack    no Absolute account — paid releases are gated");
    }

    println!("instances   {} managed", cfg.instances.len());
    Ok(())
}

// ------------------------------------------------------------------ resolving

fn resolve<'a>(cfg: &'a Config, needle: &str) -> Result<&'a Instance, String> {
    if let Some(exact) = cfg.instances.iter().find(|i| i.id == needle) {
        return Ok(exact);
    }

    let by_prefix: Vec<&Instance> = cfg
        .instances
        .iter()
        .filter(|i| i.id.starts_with(needle))
        .collect();
    if by_prefix.len() == 1 {
        return Ok(by_prefix[0]);
    }

    let by_name: Vec<&Instance> = cfg
        .instances
        .iter()
        .filter(|i| {
            i.version.eq_ignore_ascii_case(needle) || i.name.eq_ignore_ascii_case(needle)
        })
        .collect();
    if by_name.len() == 1 {
        return Ok(by_name[0]);
    }

    if by_prefix.len() > 1 || by_name.len() > 1 {
        let matches: Vec<String> = by_prefix
            .iter()
            .chain(by_name.iter())
            .map(|i| format!("  {}  {}  {}", short_id(&i.id), i.version, i.path))
            .collect();
        return Err(format!("`{needle}` matches more than one:\n{}", matches.join("\n")));
    }

    Err(format!(
        "no instance called `{needle}` — `gd list` shows what is there"
    ))
}

// ---------------------------------------------------------------------- help

fn print_help() {
    println!(
        "\
gd — GD Launcher in a terminal

  gd list                        managed instances
  gd versions                    bundled game builds
  gd install <version>           create an instance and pull its depot
  gd launch <instance>           start Geometry Dash
  gd remove <instance>           forget an instance  [--delete-files]
  gd path <instance>             print the instance folder

  gd steam                       Steam installs, accounts and the depot session
  gd import <index>              register a Steam install  [--link|--copy]
  gd login                       QR sign-in, cached for later pulls  [--force]

  gd mh <instance>               Mega Hack releases for that build
  gd mh <instance> install [r]   install one  (default: recommended)

  gd mods <instance> list        installed Geode mods
  gd mods <instance> search <q>  search the Geode index
  gd mods <instance> install <id>
  gd mods <instance> remove <id>

  gd loader <instance> [status]  Geode loader state
  gd loader <instance> install [version]

  gd doctor                      paths, toolchain and session

global
  --data-dir <path>              use a different data folder
  -v, --verbose                  show every line DepotDownloader prints

An instance is matched by full id, id prefix, version or name.
Versions come from `gd versions`; pass --manifest to pull anything else."
    );
}
