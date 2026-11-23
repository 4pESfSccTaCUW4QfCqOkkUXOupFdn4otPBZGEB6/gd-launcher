//! One depot pull, from backend lookup to child teardown.
//!
//! Events go through a sink instead of straight to stderr, so the command line
//! and the full screen interface share one code path: the CLI draws a progress
//! line, the TUI pushes the same events into a channel.

use crate::config::{DepotBackend, Settings};
use crate::{depot, paths, util};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

/// Receives everything a run produces. `Fn` and not `FnMut` so the pump threads
/// can hold it; callers put a mutex inside if they need one.
pub type Sink = Arc<dyn Fn(PullEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum PullEvent {
    /// A line aimed at the operator, not at a log.
    Note(String),
    Log {
        stream: &'static str,
        line: String,
    },
    /// Block art from a `-qr` run: the QR code itself, one frame per line.
    Qr(Vec<String>),
    Progress {
        percent: f64,
        detail: String,
    },
    /// The auto-fetched toolchain downloading.
    ToolProgress {
        percent: f64,
        received: u64,
        total: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// No account at all.
    Anonymous,
    /// Scan a code with the Steam app. Also used to refresh a dead token.
    Qr,
    /// Reuse the cached login key for this account.
    Cached { username: String },
}

impl Login {
    /// Pick what the cached session allows: a code only when nothing usable is
    /// on disk, unless the caller forces one.
    pub fn for_pull(dir: &Path, anonymous: bool, force_qr: bool) -> Self {
        if anonymous {
            return Login::Anonymous;
        }
        let cached = paths::session(dir);
        if force_qr || !cached.signed_in {
            Login::Qr
        } else {
            Login::Cached {
                username: cached.username,
            }
        }
    }
}

/// Everything one pull needs. A struct because eight positional arguments stop
/// being readable at the call site.
pub struct PullRequest<'a> {
    pub dir: &'a Path,
    pub settings: &'a Settings,
    pub target: &'a Path,
    pub depot_id: &'a str,
    pub manifest_id: &'a str,
    pub login: Login,
    /// Inherit stdin so steamcmd and a password prompt can read the terminal.
    /// The TUI passes false because it owns the terminal itself.
    pub attach_stdin: bool,
    pub sink: Sink,
}

struct Run {
    code: i32,
    tail: Vec<String>,
}

impl Run {
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

    fn last_line(&self) -> String {
        self.tail.last().cloned().unwrap_or_default()
    }
}

/// Pull one manifest into `target`, retrying once as a QR login when the cached
/// key is unusable.
pub async fn pull(req: PullRequest<'_>) -> Result<(), String> {
    let first = run(&req).await?;
    if first.ok() {
        purge_cache(req.settings, req.target);
        return Ok(());
    }

    let can_retry = matches!(req.login, Login::Cached { .. }) && first.stale_key();
    if can_retry {
        (req.sink)(PullEvent::Note(
            "the cached steam key is not readable from this install - scanning a code once".into(),
        ));
        let retry = PullRequest {
            login: Login::Qr,
            ..req
        };
        let retry = run(&retry).await?;
        if retry.ok() {
            purge_cache(req.settings, req.target);
            return Ok(());
        }
        return Err(format!(
            "DepotDownloader exited with {} - {}",
            retry.code,
            retry.last_line()
        ));
    }

    Err(format!(
        "DepotDownloader exited with {} - {}",
        first.code,
        first.last_line()
    ))
}

fn purge_cache(settings: &Settings, target: &Path) {
    if settings.purge_depot_cache {
        // A full pull leaves several GB of chunk cache behind; the instance is
        // complete once the manifest validates.
        let _ = std::fs::remove_dir_all(target.join(".DepotDownloader"));
    }
}

async fn run(req: &PullRequest<'_>) -> Result<Run, String> {
    let (backend, exe) = ensure_backend(req.dir, req.settings, &req.sink).await?;

    std::fs::create_dir_all(req.target).map_err(|e| format!("create {:?}: {e}", req.target))?;

    let (username, password, use_qr) = match &req.login {
        Login::Anonymous => (String::new(), String::new(), false),
        Login::Qr => (String::new(), String::new(), true),
        Login::Cached { username } => (username.clone(), String::new(), false),
    };

    let mut cmd = depot::build_command(
        backend,
        &exe,
        req.target,
        req.depot_id,
        req.manifest_id,
        &username,
        &password,
        use_qr,
        req.settings.max_downloads,
    );
    cmd.stdin(if req.attach_stdin {
        Stdio::inherit()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tail: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

    let out_thread = stdout.map(|stream| {
        let sink = req.sink.clone();
        let tail = tail.clone();
        std::thread::spawn(move || pump_stream(stream, "out", sink, tail))
    });
    let err_thread = stderr.map(|stream| {
        let sink = req.sink.clone();
        let tail = tail.clone();
        std::thread::spawn(move || pump_stream(stream, "err", sink, tail))
    });

    let status = child
        .wait()
        .map_err(|e| format!("wait {}: {e}", exe.display()))?;
    if let Some(handle) = out_thread {
        let _ = handle.join();
    }
    if let Some(handle) = err_thread {
        let _ = handle.join();
    }

    let recent = tail
        .lock()
        .map(|buf| {
            let skip = buf.len().saturating_sub(10);
            buf[skip..].to_vec()
        })
        .unwrap_or_default();

    Ok(Run {
        code: status.code().unwrap_or(-1),
        tail: recent,
    })
}

fn pump_stream<R: std::io::Read>(
    stream: R,
    tag: &'static str,
    sink: Sink,
    tail: Arc<std::sync::Mutex<Vec<String>>>,
) {
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
                depot::PumpEvent::Qr(lines) => sink(PullEvent::Qr(lines)),
                depot::PumpEvent::Progress { percent, detail } => {
                    sink(PullEvent::Progress { percent, detail })
                }
                depot::PumpEvent::Log { stream, line } => {
                    if let Ok(mut buf) = tail.lock() {
                        if buf.len() >= 40 {
                            buf.remove(0);
                        }
                        buf.push(line.clone());
                    }
                    sink(PullEvent::Log { stream, line });
                }
                depot::PumpEvent::AuthFailed(line) => sink(PullEvent::Note(format!(
                    "steam rejected the cached key ({line}) - run `gd login`"
                ))),
            }
        }
    }
}

/// Resolve the backend, fetching DepotDownloader on first use.
pub async fn ensure_backend(
    dir: &Path,
    settings: &Settings,
    sink: &Sink,
) -> Result<(DepotBackend, PathBuf), String> {
    match paths::resolve_backend(dir, settings) {
        Ok(pair) => Ok(pair),
        Err(err) => {
            if settings.depot_backend != DepotBackend::DepotDownloader {
                return Err(err);
            }
            let exe = fetch_depotdownloader(dir, sink).await?;
            Ok((DepotBackend::DepotDownloader, exe))
        }
    }
}

/// Download DepotDownloader into the managed tools folder.
async fn fetch_depotdownloader(dir: &Path, sink: &Sink) -> Result<PathBuf, String> {
    let tools = paths::tools_dir(dir).join("depotdownloader");
    std::fs::create_dir_all(&tools).map_err(|e| format!("create {tools:?}: {e}"))?;
    let zip = tools.join("depotdownloader.zip");

    sink(PullEvent::Note(
        "no DepotDownloader yet - fetching the 32 MB toolchain".into(),
    ));
    util::download("DepotDownloader", depot::DD_DOWNLOAD_URL, &zip, |progress| {
        sink(PullEvent::ToolProgress {
            percent: progress.percent,
            received: progress.received,
            total: progress.total,
        });
    })
    .await?;
    util::extract_zip(&zip, &tools)?;
    let _ = std::fs::remove_file(&zip);

    let exe = tools.join("DepotDownloader.exe");
    if !exe.exists() {
        return Err("archive unpacked but DepotDownloader.exe is missing".into());
    }
    Ok(exe)
}
