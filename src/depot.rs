//! Depot pulling.
//!
//! Two interchangeable backends:
//!   * SteamRE/DepotDownloader, auto-fetched and self-contained;
//!   * Valve's steamcmd, used when the user already has it.
//!
//! Both are driven as a child process. This module owns the argv, the line
//! decoder and the backend lookup; the caller owns the process, so the terminal
//! front end can stream a run however it likes.

use crate::config::{DepotBackend, GD_APP_ID};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const DD_DOWNLOAD_URL: &str =
    "https://github.com/SteamRE/DepotDownloader/releases/latest/download/DepotDownloader-windows-x64.zip";

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

/// Build the argv for the chosen backend.
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
/// art and percent lines.
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
