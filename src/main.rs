//! `gd` - GD Launcher in a terminal.
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
    geode, instance, manifests, mh, mh_install, paths, pull, steam, util,
};
use pull::{Login, PullEvent, PullRequest, Sink};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    let enable_tui = take_flag(&mut args, "-tui") | take_flag(&mut args, "--tui");
    let disable_tui = take_flag(&mut args, "--no-tui");
    // If both are given the off switch wins, which is the only way to recover from
    // a script that passes both.
    let enable_tui = enable_tui && !disable_tui;

    let dir = match data_override {
        Some(path) => PathBuf::from(path),
        None => paths::default_data_dir()?,
    };

    // The switch is persisted first, so `gd -tui` keeps working after a crash and
    // `gd --no-tui` can always get the terminal back.
    let mut cfg = Config::load(&dir);
    let mut changed = false;
    if enable_tui && !cfg.settings.tui {
        cfg.settings.tui = true;
        changed = true;
    }
    if disable_tui && cfg.settings.tui {
        cfg.settings.tui = false;
        changed = true;
    }
    if changed {
        cfg.save(&dir).map_err(|e| format!("save config: {e}"))?;
    }

    if enable_tui {
        return gd_launcher_lib::tui::run(&dir);
    }
    if disable_tui && args.is_empty() {
        println!("tui  off - bare `gd` prints help again");
        print_help();
        return Ok(());
    }

    let Some(command) = args.first().cloned() else {
        if cfg.settings.tui {
            return gd_launcher_lib::tui::run(&dir);
        }
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
        "launch" | "run" => cmd_launch(dir, args, verbose),
        "remove" | "rm" => cmd_remove(dir, args),
        "path" | "where" => cmd_path(dir, args),
        "steam" => cmd_steam(dir),
        "import" => cmd_import(dir, args, verbose),
        "login" | "signin" => cmd_login(dir, args, verbose).await,
        "mh" | "megahack" => cmd_mega_hack(dir, args, verbose).await,
        "mods" | "mod" => cmd_mods(dir, args).await,
        "loader" | "geode" => cmd_loader(dir, args, verbose).await,
        "doctor" => cmd_doctor(dir),
        "version" | "-V" | "--version" => {
            println!("gd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command `{other}` - try `gd help`")),
    }
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
/// The live line is only drawn when stderr is a terminal - piping `gd install`
/// into a file must not fill it with carriage returns and escape codes.
struct Printer {
    verbose: bool,
    line_active: bool,
    tty: bool,
    /// Last percent drawn for the auto-fetched toolchain, so the fetch prints one
    /// line per percent instead of one per chunk.
    tool_percent: f64,
}

impl Printer {
    fn new(verbose: bool) -> Self {
        use std::io::IsTerminal;
        Self {
            verbose,
            line_active: false,
            tty: std::io::stderr().is_terminal(),
            tool_percent: 0.0,
        }
    }

    fn handle(&mut self, event: PullEvent) {
        match event {
            PullEvent::Note(text) => self.note(&text),
            PullEvent::Log { stream, line } => self.log(stream, &line),
            PullEvent::Qr(lines) => self.qr(&lines),
            PullEvent::Progress { percent, detail } => self.progress(percent, &detail),
            PullEvent::ToolProgress {
                percent,
                received,
                total,
            } => self.tool_progress(percent, received, total),
        }
    }

    fn tool_progress(&mut self, percent: f64, received: u64, total: u64) {
        if !self.tty {
            return;
        }
        if percent - self.tool_percent < 1.0 && percent < 100.0 {
            return;
        }
        self.tool_percent = percent;
        eprint!(
            "\r\x1b[2K  DepotDownloader  {:>5.1}%  {} / {}",
            percent,
            util::human_bytes(received),
            if total > 0 {
                util::human_bytes(total)
            } else {
                "?".into()
            }
        );
        let _ = std::io::stderr().flush();
        self.line_active = true;
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
        if self.tool_percent > 0.0 {
            eprintln!();
            self.tool_percent = 0.0;
        }
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

// ---------------------------------------------------------------------- pull

/// Route pull events into the terminal printer. One sink, one lock, so the two
/// pump threads cannot interleave a progress line with a QR row.
fn printer_sink(printer: Arc<Mutex<Printer>>) -> Sink {
    Arc::new(move |event| {
        if let Ok(mut out) = printer.lock() {
            out.handle(event);
        }
    })
}

/// One pull to completion, drawn on stderr.
async fn pull_with_printer(
    dir: &Path,
    settings: &Settings,
    target: &Path,
    depot_id: &str,
    manifest_id: &str,
    login: Login,
    verbose: bool,
) -> Result<(), String> {
    let printer = Arc::new(Mutex::new(Printer::new(verbose)));
    let sink = printer_sink(printer.clone());
    let result = pull::pull(PullRequest {
        dir,
        settings,
        target,
        depot_id,
        manifest_id,
        login,
        attach_stdin: true,
        sink,
    })
    .await;
    if let Ok(mut out) = printer.lock() {
        out.finish();
    }
    result
}

// ------------------------------------------------------------------ commands

fn cmd_list(dir: &Path) -> Result<(), String> {
    let cfg = Config::load(dir);
    if cfg.instances.is_empty() {
        println!("no instances yet - `gd versions` then `gd install <version>`");
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
    /// Skip every question and take the defaults.
    yes: bool,
    /// Install this Mega Hack release without asking.
    mh: Option<String>,
    /// Never offer Mega Hack.
    no_mh: bool,
}

fn parse_install(args: &[String]) -> Result<InstallOpts, String> {
    let mut rest: Vec<String> = args.to_vec();
    let manifest = take_value(&mut rest, "--manifest");
    let name = take_value(&mut rest, "--name");
    let mh = take_value(&mut rest, "--mh");
    let anonymous = take_flag(&mut rest, "--anonymous") | take_flag(&mut rest, "--anon");
    let qr = take_flag(&mut rest, "--qr");
    let yes = take_flag(&mut rest, "--yes") | take_flag(&mut rest, "-y");
    let no_mh = take_flag(&mut rest, "--no-mh");

    let Some(version) = rest.into_iter().next() else {
        return Err(
            "usage: gd install <version> [--name X] [--manifest ID] [--mh RELEASE] [--no-mh] [--qr]"
                .into(),
        );
    };
    Ok(InstallOpts {
        version,
        manifest,
        name,
        anonymous,
        qr,
        yes,
        mh,
        no_mh,
    })
}

// ------------------------------------------------------------------ questions

/// One line from stdin. `None` on end of input, so a scripted run falls back to
/// the default instead of hanging.
fn read_answer() -> Option<String> {
    let mut buffer = String::new();
    match std::io::stdin().read_line(&mut buffer) {
        Ok(0) => None,
        Ok(_) => Some(buffer.trim().to_string()),
        Err(_) => None,
    }
}

fn ask(label: &str, default: &str) -> String {
    if default.is_empty() {
        eprint!("{label}: ");
    } else {
        eprint!("{label} [{default}]: ");
    }
    let _ = std::io::stderr().flush();
    match read_answer() {
        Some(answer) if !answer.is_empty() => answer,
        _ => default.to_string(),
    }
}

fn ask_yes_no(label: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    loop {
        eprint!("{label} ({hint}): ");
        let _ = std::io::stderr().flush();
        match read_answer() {
            None => return default_yes,
            Some(answer) => match answer.to_ascii_lowercase().as_str() {
                "" => return default_yes,
                "y" | "yes" => return true,
                "n" | "no" => return false,
                _ => eprintln!("  y or n"),
            },
        }
    }
}

fn pick_bundle<'a>(
    options: &[&'a mh_install::Bundle],
    answer: &str,
) -> Option<&'a mh_install::Bundle> {
    if let Ok(index) = answer.parse::<usize>() {
        if index >= 1 && index <= options.len() {
            return options.get(index - 1).copied();
        }
    }
    options
        .iter()
        .copied()
        .find(|bundle| bundle.version.eq_ignore_ascii_case(answer))
}

/// A Mega Hack release the user picked before the download started.
struct MhChoice {
    release: String,
    geode: bool,
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
                "no bundled build called {} - `gd versions` lists them, or pass --manifest",
                opts.version
            ))
        }
    };

    // Everything the run needs to know is asked up front, so a slow pull is the
    // last thing that happens and nobody is staring at a finished download
    // waiting to answer a question.
    let default_name = format!("Geometry Dash {version}");
    let name = match (&opts.name, opts.yes) {
        (Some(given), _) => given.clone(),
        (None, true) => default_name.clone(),
        (None, false) => ask("name the instance", &default_name),
    };
    let mh_choice = plan_mega_hack(dir, &version, &opts).await?;

    let mut cfg = Config::load(dir);
    let root = paths::instances_root(dir, &cfg.settings);
    std::fs::create_dir_all(&root).map_err(|e| format!("create {root:?}: {e}"))?;

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

    let cached = paths::session(dir);
    if !opts.anonymous && !opts.qr && !cached.signed_in {
        eprintln!("  no steam session - scan the code below with the Steam app");
    }
    let login = Login::for_pull(dir, opts.anonymous, opts.qr);

    pull_with_printer(
        dir,
        &cfg.settings,
        &target,
        &record.depot_id,
        &manifest,
        login,
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

    if let Some(choice) = mh_choice {
        apply_mega_hack(dir, &record, &choice, verbose).await;
    }

    println!("launch    gd launch {}", short_id(&record.id));
    Ok(())
}

/// Decide which Mega Hack release goes on the instance, before anything is
/// downloaded.
///
/// Only offered below 2.2, where Geode does not exist yet. A fresh instance has
/// no loader on disk, so only standalone packages can be picked here; the Geode
/// ones need `gd loader` first. The list comes from Absolute's own catalogue, so
/// a build with no package says so and points at the store instead of being
/// handed the wrong DLL.
async fn plan_mega_hack(
    dir: &Path,
    version: &str,
    opts: &InstallOpts,
) -> Result<Option<MhChoice>, String> {
    if opts.no_mh {
        return Ok(None);
    }
    // --yes means take the defaults, and the default answer here is no. Skip the
    // catalogue fetch rather than pulling it down to decide nothing.
    if opts.yes && opts.mh.is_none() {
        return Ok(None);
    }

    let below = mh::below_2_2(version);
    let forced = opts.mh.is_some();
    if !below && !forced {
        return Ok(None);
    }

    let bundles = mh_install::catalog(&paths::mh_cache(dir))
        .await
        .unwrap_or_default();
    let for_build: Vec<&mh_install::Bundle> = bundles
        .iter()
        .filter(|bundle| bundle.gjver == version)
        .collect();
    let available: Vec<&mh_install::Bundle> =
        for_build.iter().copied().filter(|b| !b.geode).collect();

    if available.is_empty() {
        if forced {
            return Err(format!("mega hack has no package for {version}"));
        }
        eprintln!();
        if for_build.is_empty() {
            eprintln!("mega hack  no package for {version}");
            if let Some(closest) = mh::recommended(version) {
                eprintln!(
                    "           closest is {} for {}; get it from https://absolllute.com/store",
                    closest.version, closest.game
                );
            }
        } else {
            eprintln!("mega hack  {version} only has a Geode package");
            eprintln!("           gd loader <instance> install, then gd mh <instance> install");
        }
        return Ok(None);
    }

    let licence = mh_install::license_status().signed_in;
    let default_bundle = mh_install::pick_for(&bundles, version, "", false);

    if let Some(name) = &opts.mh {
        let Some(found) = available
            .iter()
            .copied()
            .find(|bundle| bundle.version.eq_ignore_ascii_case(name))
        else {
            return Err(format!("mega hack {name} is not published for {version}"));
        };
        return Ok(Some(MhChoice {
            release: found.version.clone(),
            geode: found.geode,
        }));
    }

    if opts.yes {
        return Ok(None);
    }

    eprintln!();
    if !ask_yes_no(&format!("install mega hack for {version}?"), false) {
        return Ok(None);
    }
    eprintln!();
    for (index, bundle) in available.iter().enumerate() {
        let channel = if bundle.geode { "geode" } else { "standalone" };
        let cost = match (bundle.paid(), licence) {
            (false, _) => "free",
            (true, true) => "paid",
            (true, false) => "paid, needs an Absolute account",
        };
        let is_default = default_bundle
            .map(|d| d.version == bundle.version && d.file == bundle.file)
            .unwrap_or(false);
        eprintln!(
            "  {}) {:<16} {:<10} {}{}",
            index + 1,
            bundle.version,
            channel,
            cost,
            if is_default { "  <- default" } else { "" }
        );
    }
    let fallback = default_bundle
        .map(|bundle| bundle.version.clone())
        .unwrap_or_else(|| available[0].version.clone());
    loop {
        let answer = ask("release", &fallback);
        let Some(picked) = pick_bundle(&available, &answer) else {
            eprintln!("  no release called `{answer}` in that list");
            continue;
        };
        if picked.paid() && !licence {
            eprintln!(
                "  {} is paid and needs an Absolute account; pick a free one or sign in first",
                picked.version
            );
            continue;
        }
        return Ok(Some(MhChoice {
            release: picked.version.clone(),
            geode: picked.geode,
        }));
    }
}

/// Drop the chosen release into an instance that just finished downloading.
/// A failure here leaves a playable build, so it warns instead of aborting.
async fn apply_mega_hack(dir: &Path, record: &Instance, choice: &MhChoice, verbose: bool) {
    let path = PathBuf::from(&record.path);
    let mut printer = Printer::new(verbose);
    let report = mh_install::install(
        &paths::mh_cache(dir),
        &path,
        &record.version,
        &choice.release,
        choice.geode,
        |progress| printer.progress(progress.percent, &progress.label),
    )
    .await;
    printer.finish();

    match report {
        Ok(report) => println!(
            "megahack  {}  {} files  {}",
            report.bundle.version,
            report.files.len(),
            util::human_bytes(report.bytes)
        ),
        Err(err) => eprintln!("  mega hack skipped: {err}"),
    }
}

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .count()
}

fn cmd_launch(dir: &Path, args: &[String], verbose: bool) -> Result<(), String> {
    let needle = args.first().map(|s| s.as_str()).unwrap_or("steam");
    let cfg = Config::load(dir);

    // `gd launch`, `gd launch steam` and `gd launch default` all start the copy
    // Steam installed. Anything else is matched against the managed instances.
    if needle.is_empty() || needle.eq_ignore_ascii_case("steam") || needle.eq_ignore_ascii_case("default")
    {
        return launch_steam(verbose, &cfg.settings.game_args);
    }

    let record = resolve(&cfg, needle)?;
    let path = Path::new(&record.path);
    if !path.join("GeometryDash.exe").exists() {
        return Err(format!("{} has no GeometryDash.exe", path.display()));
    }
    instance::launch_path_with(path, !verbose, &cfg.settings.game_args)?;
    println!("launched  {}  {}", short_id(&record.id), record.version);
    Ok(())
}

/// Start the game where Steam installed it. No `steam_appid.txt` is written: the
/// Steam copy already sits inside its own library, so Steam is the one launching
/// it and the app id is not needed.
fn launch_steam(verbose: bool, game_args: &str) -> Result<(), String> {
    let installs = steam::find_installs();
    if installs.is_empty() {
        return Err("no Steam copy found - `gd steam` shows what the launcher can see".into());
    }

    let mut stale: Vec<String> = Vec::new();
    for install in &installs {
        let path = PathBuf::from(&install.path);
        if !steam::looks_like_gd(&path) {
            stale.push(install.path.clone());
            continue;
        }
        instance::launch_path_with(&path, !verbose, game_args)?;
        println!(
            "launched  steam  build {}  {}",
            if install.build_id.is_empty() {
                "?"
            } else {
                &install.build_id
            },
            install.path
        );
        return Ok(());
    }

    Err(format!(
        "Steam has Geometry Dash registered but no GeometryDash.exe on disk: {}",
        stale.join(", ")
    ))
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

    let cached = paths::session(dir);
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
        println!("  no cached login - `gd login` scans a QR once");
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
            "{} has no GeometryDash.exe - that library entry is stale",
            source.display()
        ));
    }

    let mut cfg = Config::load(dir);
    let root = paths::instances_root(dir, &cfg.settings);
    let label = name.unwrap_or_else(|| "Steam copy".to_string());

    // Copying is the safe default: Steam keeps updating its own folder.
    let (path, linked) = if link && !copy {
        (source.clone(), true)
    } else {
        let target = instance::unique_dir(&root, &label, &install.build_id);
        eprintln!("  copying {} - this takes a minute", source.display());
        let tty = {
            use std::io::IsTerminal;
            std::io::stderr().is_terminal()
        };
        instance::copy_tree(&source, &target, |progress| {
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

    let cached = paths::session(dir);
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
        "  signing in - this pulls Geometry Dash {} once (~40 MB), then deletes it",
        smallest.version
    );

    let cfg = Config::load(dir);
    pull_with_printer(
        dir,
        &cfg.settings,
        &scratch,
        config::GD_WIN_DEPOT,
        smallest.manifest,
        Login::Qr,
        verbose,
    )
    .await?;

    let _ = std::fs::remove_dir_all(&scratch);
    let after = paths::session(dir);
    if after.signed_in {
        println!("signed in  {}", after.username);
    } else {
        println!("pull finished but no token was written - check the Steam app");
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
                "no Absolute account - paid releases will not install".into()
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
    let report = mh_install::install(
        &paths::mh_cache(dir),
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
                return Err("Geode is not installed here - `gd loader <instance> install`".into());
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
    let version = geode::install_loader(&path, requested, &paths::cache_dir(dir), |progress| {
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
        paths::instances_root(dir, &cfg.settings).display()
    );
    println!(
        "backend     {}",
        match cfg.settings.depot_backend {
            DepotBackend::DepotDownloader => "depotdownloader",
            DepotBackend::SteamCmd => "steamcmd",
        }
    );

    match paths::resolve_backend(dir, &cfg.settings) {
        Ok((_, exe)) => println!("toolchain   {}", exe.display()),
        Err(err) => println!("toolchain   {err} - it is fetched on first install"),
    }

    let cached = paths::session(dir);
    if cached.signed_in {
        println!("session     {} ({})", cached.username, cached.path);
    } else {
        println!("session     none - `gd login`");
    }

    let license = mh_install::license_status();
    if license.signed_in {
        println!("megahack    licence {} on disk", license.masked);
    } else {
        println!("megahack    no Absolute account - paid releases are gated");
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
        "no instance called `{needle}` - `gd list` shows what is there"
    ))
}

// ---------------------------------------------------------------------- help

fn print_help() {
    println!(
        "\
gd - GD Launcher in a terminal

  gd list                        managed instances
  gd versions                    bundled game builds
  gd install <version>           create an instance and pull its depot
  gd launch [instance]           start Geometry Dash  (no name: the Steam copy)
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

  gd -tui                        full screen interface, remembered after that
  gd --no-tui                    stop bare `gd` opening it

install asks two things: what to call the instance, and for builds below 2.2,
whether to put Mega Hack on it and which release.
  --name <text>                  skip the name question
  --mh <release>                 install that Mega Hack release, no questions
  --no-mh                        never offer Mega Hack
  -y, --yes                      take every default, ask nothing

gd launch with no name, `steam` or `default` starts the copy Steam installed.
Any other name matches a managed instance.

global
  --data-dir <path>              use a different data folder
  -v, --verbose                  show every line DepotDownloader prints

The full screen interface has six screens: instances, versions, mods, mega hack,
steam and settings. Settings holds the palette, the tui default, the depot
backend, download count, cache purge, the instances folder and game arguments.

An instance is matched by full id, id prefix, version or name.
Versions come from `gd versions`; pass --manifest to pull anything else."
    );
}
