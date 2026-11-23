//! Depot pulling.
//!
//! Two interchangeable backends:
//!   * SteamRE/DepotDownloader — auto-fetched, self-contained, streams progress;
//!   * Valve's steamcmd — used when the user already has it.
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
            Err("DepotDownloader is not installed yet — press Install toolchain".into())
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
            Err("steamcmd.exe not found — set its path in Settings or switch backend".into())
        }
    }
}

fn find_steamcmd() -> Option<PathBuf> {
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

/// Build the argv for the chosen backend.
#[allow(clippy::too_many_arguments)]
fn build_command(
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
                // Cancelled from the UI — the cancel path already emitted.
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

/// Read a child stream line by line, forwarding text and scraping progress.
///
/// QR logins arrive as block-character art. Those lines are pulled out of the
/// normal log and republished as `depot://qr` so the UI can render a real, square
/// code instead of a squashed terminal one.
fn pump<R: std::io::Read + Send + 'static>(app: AppHandle, stream: R, tag: &'static str) {
    let mut reader = BufReader::new(stream);
    let mut raw: Vec<u8> = Vec::new();
    let mut qr: Vec<String> = Vec::new();
    let mut capturing = false;
    // The quiet zone above a QR is printed as blank lines *before* the first block
    // row, so they have to be buffered in case a code starts.
    let mut pending: Vec<String> = Vec::new();

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
        let line = decode_console_line(&raw);
        let has_block = line.chars().any(is_block_char);
        let blankish = line.trim().is_empty();

        if capturing {
            if has_block || blankish {
                qr.push(line);
                if qr.len() > 96 {
                    qr.clear();
                    capturing = false;
                } else {
                    let _ = app.emit("depot://qr", QrFrame { lines: qr.clone() });
                }
                continue;
            }
            capturing = false;
            qr.clear();
        }

        if has_block {
            capturing = true;
            qr = std::mem::take(&mut pending);
            qr.push(line);
            let _ = app.emit("depot://qr", QrFrame { lines: qr.clone() });
            continue;
        }

        if blankish {
            pending.push(line);
            if pending.len() > 8 {
                pending.remove(0);
            }
            continue;
        }

        pending.clear();
        let _ = app.emit(
            "depot://log",
            DepotLog {
                stream: tag.into(),
                line: line.clone(),
            },
        );
        if auth_failed(&line) {
            // The stored key is dead. Drop it so the next launch offers the QR
            // again instead of silently failing every pull.
            let extra = crate::session_paths(&app);
            depot_session::forget(&extra);
            let _ = app.emit("steam://auth-needed", line.clone());
        }
        if let Some(percent) = parse_percent(&line) {
            let _ = app.emit(
                "depot://progress",
                DepotProgress {
                    percent,
                    detail: line.trim().to_string(),
                },
            );
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

/// Feed a line to the running process — Steam Guard codes, password prompts.
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
