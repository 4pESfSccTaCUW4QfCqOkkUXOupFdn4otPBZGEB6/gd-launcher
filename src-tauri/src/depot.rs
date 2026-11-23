//! Depot pulling.
//!
//! Two interchangeable backends:
//!   * SteamRE/DepotDownloader - auto-fetched, self-contained, streams progress;
//!   * Valve's steamcmd - used when the user already has it.
//!
//! Both are driven as a child process with piped stdio so Steam Guard prompts can
//! be answered from the UI and every line of output lands in the launcher log.

use crate::config::{DepotBackend, GD_APP_ID};
use crate::depot_session;
use crate::state::{AppState, RunningJob};
use crate::util;
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const DD_DOWNLOAD_URL: &str =
    "https://github.com/SteamRE/DepotDownloader/releases/latest/download/DepotDownloader-windows-x64.zip";

#[derive(Debug, Clone, Serialize)]
pub struct ToolchainStatus {
    pub backend: DepotBackend,
    pub depotdownloader: Option<String>,
    pub steamcmd: Option<String>,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotLog {
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotProgress {
    pub percent: f64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotExit {
    pub code: i32,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct QrFrame {
    pub lines: Vec<String>,
}

pub fn tools_dir(app: &AppHandle) -> PathBuf {
    crate::config::data_root(app).join("tools")
}

pub fn managed_depotdownloader(app: &AppHandle) -> PathBuf {
    tools_dir(app).join("depotdownloader").join("DepotDownloader.exe")
}

/// Resolve whichever backend the user has. Overrides win over managed copies.
pub fn resolve_backend(app: &AppHandle, state: &AppState) -> Result<(DepotBackend, PathBuf), String> {
    let cfg = state.snapshot()?;
    let settings = &cfg.settings;
    match settings.depot_backend {
        DepotBackend::DepotDownloader => {
            if !settings.depotdownloader_path.trim().is_empty() {
                let p = PathBuf::from(settings.depotdownloader_path.trim());
                if p.exists() {
                    return Ok((DepotBackend::DepotDownloader, p));
                }
            }
            let managed = managed_depotdownloader(app);
            if managed.exists() {
                return Ok((DepotBackend::DepotDownloader, managed));
            }
            Err("DepotDownloader is not installed yet - press Install toolchain".into())
        }
        DepotBackend::SteamCmd => {
            if !settings.steamcmd_path.trim().is_empty() {
                let p = PathBuf::from(settings.steamcmd_path.trim());
                if p.exists() {
                    return Ok((DepotBackend::SteamCmd, p));
                }
            }
            if let Some(found) = find_steamcmd() {
                return Ok((DepotBackend::SteamCmd, found));
            }
            Err("steamcmd.exe not found - set its path in Settings or switch backend".into())
        }
    }
}

pub fn find_steamcmd() -> Option<PathBuf> {
    for candidate in [
        r"C:\steamcmd\steamcmd.exe",
        r"C:\Program Files (x86)\Steam\steamcmd.exe",
        r"C:\Program Files\Steam\steamcmd.exe",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    // PATH lookup.
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("steamcmd.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn toolchain_status(app: &AppHandle, state: &AppState) -> Result<ToolchainStatus, String> {
    let cfg = state.snapshot()?;
    let dd_override = PathBuf::from(cfg.settings.depotdownloader_path.trim());
    let dd = if !cfg.settings.depotdownloader_path.trim().is_empty() && dd_override.exists() {
        Some(dd_override)
    } else {
        let managed = managed_depotdownloader(app);
        managed.exists().then_some(managed)
    };
    let sc_override = PathBuf::from(cfg.settings.steamcmd_path.trim());
    let sc = if !cfg.settings.steamcmd_path.trim().is_empty() && sc_override.exists() {
        Some(sc_override)
    } else {
        find_steamcmd()
    };
    let ready = match cfg.settings.depot_backend {
        DepotBackend::DepotDownloader => dd.is_some(),
        DepotBackend::SteamCmd => sc.is_some(),
    };
    Ok(ToolchainStatus {
        backend: cfg.settings.depot_backend,
        depotdownloader: dd.map(|p| p.display().to_string()),
        steamcmd: sc.map(|p| p.display().to_string()),
        ready,
    })
}

/// Fetch and unpack DepotDownloader into the managed tools folder.
pub async fn install_depotdownloader(app: AppHandle) -> Result<String, String> {
    let dir = tools_dir(&app).join("depotdownloader");
    let zip_path = dir.join("depotdownloader.zip");
    util::download_to_file(&app, "tool://progress", "DepotDownloader", DD_DOWNLOAD_URL, &zip_path)
        .await?;
    util::extract_zip(&zip_path, &dir)?;
    let _ = std::fs::remove_file(&zip_path);
    let exe = dir.join("DepotDownloader.exe");
    if !exe.exists() {
        return Err("archive unpacked but DepotDownloader.exe is missing".into());
    }
    Ok(exe.display().to_string())
}

/// Build the argv for the chosen backend. Public so the CLI drives the tool with
/// exactly the same flags the window does.
#[allow(clippy::too_many_arguments)]
pub fn build_command(
    backend: DepotBackend,
    exe: &Path,
    install_dir: &Path,
    depot_id: &str,
    manifest_id: &str,
    username: &str,
    password: &str,
    use_qr: bool,
    max_downloads: u32,
) -> Command {
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Keep the tool's own login cache next to the tool, not inside the instance,
    // so one authenticated session covers every version.
    if let Some(parent) = exe.parent() {
        cmd.current_dir(parent);
    }

    match backend {
        DepotBackend::DepotDownloader => {
            cmd.arg("-app").arg(GD_APP_ID);
            cmd.arg("-depot").arg(depot_id);
            if !manifest_id.trim().is_empty() {
                cmd.arg("-manifest").arg(manifest_id.trim());
            }
            cmd.arg("-dir").arg(install_dir);
            cmd.arg("-max-downloads").arg(max_downloads.clamp(1, 32).to_string());
            if use_qr {
                // Phone scan, no password. The login key is persisted so the next
                // pull authenticates silently.
                cmd.arg("-qr");
                cmd.arg("-remember-password");
            } else if username.trim().is_empty() {
                cmd.arg("-anonymous");
            } else {
                cmd.arg("-username").arg(username.trim());
                if !password.is_empty() {
                    cmd.arg("-password").arg(password);
                }
                // Reuses the cached login key when there is one; only prompts for a
                // password on the very first run.
                cmd.arg("-remember-password");
            }
            cmd.arg("-validate");
        }
        DepotBackend::SteamCmd => {
            cmd.arg("+force_install_dir").arg(install_dir);
            if username.trim().is_empty() {
                cmd.arg("+login").arg("anonymous");
            } else {
                cmd.arg("+login").arg(username.trim()).arg(password);
            }
            cmd.arg("+app_update").arg(GD_APP_ID);
            cmd.arg("-depot").arg(depot_id);
            if !manifest_id.trim().is_empty() {
                cmd.arg("-manifest").arg(manifest_id.trim());
            }
            cmd.arg("validate");
            cmd.arg("+quit");
        }
    }
    cmd
}

/// Pull a manifest into an instance folder. Returns immediately; progress and
/// output arrive as events, completion as `depot://exit`.
#[allow(clippy::too_many_arguments)]
pub fn start_download(
    app: AppHandle,
    instance_path: String,
    depot_id: String,
    manifest_id: String,
    username: String,
    password: String,
    use_qr: bool,
) -> Result<(), String> {
    let state = app.state::<AppState>();

    {
        let guard = state.job.lock().map_err(|_| "job lock poisoned")?;
        if guard.is_some() {
            return Err("a depot download is already running".into());
        }
    }

    let (backend, exe) = resolve_backend(&app, &state)?;
    let cfg = state.snapshot()?;
    let install_dir = PathBuf::from(&instance_path);
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("create {install_dir:?}: {e}"))?;

    let mut cmd = build_command(
        backend,
        &exe,
        &install_dir,
        &depot_id,
        &manifest_id,
        &username,
        &password,
        use_qr,
        cfg.settings.max_downloads,
    );
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(out) = stdout {
        let app_out = app.clone();
        std::thread::spawn(move || pump(app_out, out, "out"));
    }
    if let Some(err) = stderr {
        let app_err = app.clone();
        std::thread::spawn(move || pump(app_err, err, "err"));
    }

    {
        let mut guard = state.job.lock().map_err(|_| "job lock poisoned")?;
        *guard = Some(RunningJob {
            child,
            stdin,
            used_qr: use_qr,
        });
    }

    // Watcher thread: reaps the child and reports the exit code.
    let app_done = app.clone();
    let watched_dir = install_dir.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let state = app_done.state::<AppState>();
        let mut finished: Option<i32> = None;
        let mut used_qr = false;
        {
            let Ok(mut guard) = state.job.lock() else { return };
            let Some(job) = guard.as_mut() else {
                // Cancelled from the UI - the cancel path already emitted.
                return;
            };
            match job.child.try_wait() {
                Ok(Some(status)) => {
                    finished = Some(status.code().unwrap_or(-1));
                    used_qr = job.used_qr;
                    *guard = None;
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = app_done.emit(
                        "depot://log",
                        DepotLog {
                            stream: "err".into(),
                            line: format!("wait: {e}"),
                        },
                    );
                    *guard = None;
                    return;
                }
            }
        }
        if let Some(code) = finished {
            if code == 0 {
                let state = app_done.state::<AppState>();
                if let Ok(cfg) = state.snapshot() {
                    if cfg.settings.purge_depot_cache {
                        // DepotDownloader leaves a chunk cache behind; several GB after a
                        // full pull. The instance is complete once the manifest validates.
                        let _ = std::fs::remove_dir_all(watched_dir.join(".DepotDownloader"));
                    }
                }
                // A clean QR run means the login key is now cached. Stop offering
                // the scan flow from here on.
                if used_qr {
                    if let Ok(mut cfg) = state.config.lock() {
                        if !cfg.settings.steam_qr_done {
                            cfg.settings.steam_qr_done = true;
                            let dir = state.data_dir.clone();
                            let _ = cfg.save(&dir);
                        }
                    }
                    let _ = app_done.emit("steam://qr-done", true);
                }
            }
            let _ = app_done.emit(
                "depot://exit",
                DepotExit {
                    code,
                    ok: code == 0,
                },
            );
            return;
        }
    });

    Ok(())
}

/// Console output from DepotDownloader arrives in the Windows OEM code page, so
/// the QR block glyph is byte 0xDB rather than UTF-8. Decode it back to the
/// Unicode block characters before anything looks at it.
fn decode_console_line(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7F => b as char,
            0xB0 => '\u{2591}',
            0xB1 => '\u{2592}',
            0xB2 => '\u{2593}',
            0xDB => '\u{2588}',
            0xDC => '\u{2584}',
            0xDD => '\u{258C}',
            0xDE => '\u{2590}',
            0xDF => '\u{2580}',
            _ => ' ',
        })
        .collect()
}

fn is_block_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{2588}' | '\u{2580}' | '\u{2584}' | '\u{258C}' | '\u{2590}' | '\u{2591}' | '\u{2592}'
            | '\u{2593}'
    )
}

/// Lines that mean the cached login key is dead. Steam reports them on stderr when
/// a remembered token has expired or been revoked.
fn auth_failed(line: &str) -> bool {
    const MARKERS: [&str; 6] = [
        "InvalidPassword",
        "InvalidLoginAuthCode",
        "TwoFactorCodeMismatch",
        "AccountLoginDeniedNeedTwoFactor",
        "LoggedInElsewhere",
        "AccessDenied",
    ];
    MARKERS.iter().any(|marker| line.contains(marker))
}

/// What one line of child output means.
pub enum PumpEvent {
    /// Ordinary output worth showing in a log.
    Log {
        stream: &'static str,
        line: String,
    },
    /// Block art from a `-qr` run: the QR code itself.
    Qr(Vec<String>),
    /// A download progress line.
    Progress { percent: f64, detail: String },
    /// Steam rejected the cached key.
    AuthFailed(String),
}

/// Line decoder for DepotDownloader and steamcmd output: OEM code page, QR block
/// art and percent lines. Pure, so the window and the CLI read a run the same way.
#[derive(Default)]
pub struct LinePump {
    qr: Vec<String>,
    capturing: bool,
    /// The quiet zone above a QR is printed as blank lines *before* the first
    /// block row, so they are buffered in case a code starts.
    pending: Vec<String>,
}

impl LinePump {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one raw line (newline already stripped) and get back what it means.
    /// `stream` tags log lines: "out" or "err".
    pub fn feed(&mut self, raw: &[u8], stream: &'static str) -> Vec<PumpEvent> {
        let line = decode_console_line(raw);
        let has_block = line.chars().any(is_block_char);
        let blankish = line.trim().is_empty();

        if self.capturing {
            if has_block || blankish {
                self.qr.push(line);
                if self.qr.len() > 96 {
                    self.qr.clear();
                    self.capturing = false;
                    return Vec::new();
                }
                return vec![PumpEvent::Qr(self.qr.clone())];
            }
            self.capturing = false;
            self.qr.clear();
        }

        if has_block {
            self.capturing = true;
            self.qr = std::mem::take(&mut self.pending);
            self.qr.push(line);
            return vec![PumpEvent::Qr(self.qr.clone())];
        }

        if blankish {
            self.pending.push(line);
            if self.pending.len() > 8 {
                self.pending.remove(0);
            }
            return Vec::new();
        }

        self.pending.clear();
        let mut out = Vec::with_capacity(3);
        if auth_failed(&line) {
            // The stored key is dead. Report it before the log line so the caller
            // can drop the token and offer a fresh code.
            out.push(PumpEvent::AuthFailed(line.clone()));
        }
        if let Some(percent) = parse_percent(&line) {
            out.push(PumpEvent::Progress {
                percent,
                detail: line.trim().to_string(),
            });
        }
        out.push(PumpEvent::Log {
            stream,
            line,
        });
        out
    }
}

/// Read a child stream line by line, forwarding text and scraping progress.
///
/// QR logins arrive as block-character art. Those lines are pulled out of the
/// normal log and republished as `depot://qr` so the UI can render a real, square
/// code instead of a squashed terminal one.
fn pump<R: std::io::Read + Send + 'static>(app: AppHandle, stream: R, tag: &'static str) {
    let mut reader = BufReader::new(stream);
    let mut raw: Vec<u8> = Vec::new();
    let mut decoder = LinePump::new();

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
                PumpEvent::Qr(lines) => {
                    let _ = app.emit("depot://qr", QrFrame { lines });
                }
                PumpEvent::Log { stream, line } => {
                    let _ = app.emit(
                        "depot://log",
                        DepotLog {
                            stream: stream.into(),
                            line,
                        },
                    );
                }
                PumpEvent::Progress { percent, detail } => {
                    let _ = app.emit("depot://progress", DepotProgress { percent, detail });
                }
                PumpEvent::AuthFailed(line) => {
                    // Drop the dead token so the next launch offers the QR again
                    // instead of silently failing every pull.
                    let extra = crate::session_paths(&app);
                    depot_session::forget(&extra);
                    let _ = app.emit("steam://auth-needed", line);
                }
            }
        }
    }
}

/// DepotDownloader prints `  42.17% 1,234,567 / 2,900,000`; steamcmd prints
/// `Update state (0x61) downloading, progress: 42.17 (1234 / 2900)`.
fn parse_percent(line: &str) -> Option<f64> {
    if let Some(idx) = line.find('%') {
        let head = &line[..idx];
        let token: String = head
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if let Ok(v) = token.parse::<f64>() {
            return Some(v.clamp(0.0, 100.0));
        }
    }
    if let Some(idx) = line.find("progress:") {
        let rest = line[idx + "progress:".len()..].trim();
        let token: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(v) = token.parse::<f64>() {
            return Some(v.clamp(0.0, 100.0));
        }
    }
    None
}

/// Feed a line to the running process - Steam Guard codes, password prompts.
pub fn send_input(app: &AppHandle, text: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut guard = state.job.lock().map_err(|_| "job lock poisoned")?;
    let job = guard.as_mut().ok_or("no download is running")?;
    let stdin = job.stdin.as_mut().ok_or("process stdin is closed")?;
    writeln!(stdin, "{}", text.trim()).map_err(|e| format!("write stdin: {e}"))?;
    stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
    Ok(())
}

/// Kill the running download. Partial files stay on disk; DepotDownloader
/// resumes cleanly on the next attempt.
pub fn cancel(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut guard = state.job.lock().map_err(|_| "job lock poisoned")?;
    if let Some(mut job) = guard.take() {
        let _ = job.child.kill();
        let _ = job.child.wait();
    }
    let _ = app.emit(
        "depot://exit",
        DepotExit {
            code: -2,
            ok: false,
        },
    );
    Ok(())
}

pub fn is_running(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state
        .job
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&[u8]]) -> Vec<PumpEvent> {
        let mut pump = LinePump::new();
        lines.iter().flat_map(|line| pump.feed(line, "out")).collect()
    }

    #[test]
    fn qr_art_is_captured_not_logged() {
        // DepotDownloader writes the code in the OEM code page: 0xDB is a full
        // block, not UTF-8.
        let events = feed(&[
            b"Use the Steam Mobile App to sign in with this QR code:",
            b"",
            &[0xDB, 0xDB, 0x20, 0xDB],
            &[0x20, 0xDB, 0xDB, 0x20],
        ]);

        let frames: Vec<&Vec<String>> = events
            .iter()
            .filter_map(|event| match event {
                PumpEvent::Qr(lines) => Some(lines),
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 2, "one frame per captured line");

        let last = frames.last().unwrap();
        // The blank line above the code comes along as the quiet zone.
        assert_eq!(last.len(), 3);
        assert_eq!(last[1], "\u{2588}\u{2588} \u{2588}");
        assert_eq!(last[2], " \u{2588}\u{2588} ");

        // Nothing from the code may leak into the log stream.
        assert!(!events.iter().any(|event| matches!(
            event,
            PumpEvent::Log { line, .. } if line.contains('\u{2588}')
        )));
    }

    #[test]
    fn percent_lines_become_progress() {
        let events = feed(&[b"  42.17% 1,234,567 / 2,900,000"]);
        let percent = events.iter().find_map(|event| match event {
            PumpEvent::Progress { percent, .. } => Some(*percent),
            _ => None,
        });
        assert_eq!(percent, Some(42.17));
    }

    #[test]
    fn steamcmd_progress_is_read_too() {
        let events = feed(&[b"Update state (0x61) downloading, progress: 12.5 (100 / 800)"]);
        let percent = events.iter().find_map(|event| match event {
            PumpEvent::Progress { percent, .. } => Some(*percent),
            _ => None,
        });
        assert_eq!(percent, Some(12.5));
    }

    #[test]
    fn a_rejected_key_is_reported_before_the_log_line() {
        let events = feed(&[b"InvalidPassword"]);
        assert!(matches!(events.first(), Some(PumpEvent::AuthFailed(_))));
        assert!(matches!(events.last(), Some(PumpEvent::Log { .. })));
    }
}
