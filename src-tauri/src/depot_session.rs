//! Whether DepotDownloader already holds a Steam login token.
//!
//! `-qr -remember-password` does not drop anything next to the exe. .NET isolated
//! storage puts it here:
//!
//!   %LOCALAPPDATA%\IsolatedStorage\<user>\<assembly>\<domain>\AssemFiles\account.config
//!
//! The assembly and domain segments are hashes of the assembly identity, so they
//! change with every DepotDownloader release — the file has to be found by walking
//! the store, never by a fixed path.
//!
//! The file is a raw-deflate-wrapped protobuf `AccountSettingsStore`. The login
//! tokens are field 4, a `map<string, string>` where each entry is a message of
//! field 1 (account name) and field 2 (the refresh token). Only the key and the
//! presence of the token are ever read; the token itself never leaves this module.
//!
//! This is the truth about the session. A config flag saying "the QR was done once"
//! is a lie the moment the token expires, so the UI asks here instead.

use flate2::read::DeflateDecoder;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DepotSession {
    pub signed_in: bool,
    /// Steam account name the cached token belongs to.
    pub username: String,
    /// The store file the token was read from, for the settings page.
    pub path: String,
    /// Unix seconds — when DepotDownloader last wrote the token.
    pub updated_at: i64,
}

impl Default for DepotSession {
    fn default() -> Self {
        Self {
            signed_in: false,
            username: String::new(),
            path: String::new(),
            updated_at: 0,
        }
    }
}

const FILE_NAME: &str = "account.config";

/// Every `account.config` the .NET isolated store could be holding, plus any
/// explicit paths the caller wants checked (the managed tools folder, in case a
/// future DepotDownloader stops using isolated storage).
fn candidates(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        walk(&PathBuf::from(base).join("IsolatedStorage"), 0, &mut out);
    }
    // Older and future layouts: a plain folder beside the app.
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let plain = PathBuf::from(base).join("DepotDownloader").join(FILE_NAME);
            if plain.is_file() {
                out.push(plain);
            }
        }
    }
    for path in extra {
        if path.is_file() {
            out.push(path.clone());
        }
    }
    out
}

/// Depth-bounded walk. The store is five levels deep at most; anything deeper is
/// not a store file and is not worth the I/O.
fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, depth + 1, out);
        } else if kind.is_file() && entry.file_name().eq_ignore_ascii_case(FILE_NAME) {
            out.push(path);
        }
    }
}

/// The newest store file that actually contains a login token.
pub fn cached_session(extra: &[PathBuf]) -> Option<DepotSession> {
    let mut best: Option<DepotSession> = None;

    for path in candidates(extra) {
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Some(blob) = inflate(&raw) else {
            continue;
        };
        let Some((username, _token)) = first_token(&blob) else {
            continue;
        };
        let updated_at = path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let session = DepotSession {
            signed_in: true,
            username,
            path: path.display().to_string(),
            updated_at,
        };
        if best
            .as_ref()
            .map(|current| updated_at > current.updated_at)
            .unwrap_or(true)
        {
            best = Some(session);
        }
    }
    best
}

/// Delete every store file that holds a token. Used by "reset session" and when a
/// pull comes back with a dead key — a token Steam has rejected is rubbish, and
/// keeping it only makes the UI claim a session that is not there.
pub fn forget(extra: &[PathBuf]) -> usize {
    let mut removed = 0usize;
    for path in candidates(extra) {
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Some(blob) = inflate(&raw) else {
            continue;
        };
        if first_token(&blob).is_some() && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn inflate(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(raw).read_to_end(&mut out).ok()?;
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

/// `(account, token)` from the first login-token entry with a non-empty token.
fn first_token(blob: &[u8]) -> Option<(String, String)> {
    let mut pos = 0usize;
    while pos < blob.len() {
        let tag = read_varint(blob, &mut pos)?;
        let field = tag >> 3;
        let wire = tag & 7;
        match wire {
            // varint — skip the value.
            0 => {
                read_varint(blob, &mut pos)?;
            }
            // 64-bit fixed.
            1 => pos = pos.checked_add(8)?,
            // length-delimited.
            2 => {
                let len = read_varint(blob, &mut pos)? as usize;
                let end = pos.checked_add(len)?;
                let slice = blob.get(pos..end)?;
                pos = end;
                if field == 4 {
                    if let Some(pair) = token_entry(slice) {
                        return Some(pair);
                    }
                }
            }
            // 32-bit fixed.
            5 => pos = pos.checked_add(4)?,
            // Groups are not used by this schema.
            _ => return None,
        }
        if pos > blob.len() {
            return None;
        }
    }
    None
}

fn token_entry(entry: &[u8]) -> Option<(String, String)> {
    let mut pos = 0usize;
    let mut name = String::new();
    let mut token = String::new();
    while pos < entry.len() {
        let tag = read_varint(entry, &mut pos)?;
        if tag & 7 != 2 {
            return None;
        }
        let field = tag >> 3;
        let len = read_varint(entry, &mut pos)? as usize;
        let end = pos.checked_add(len)?;
        let slice = entry.get(pos..end)?;
        pos = end;
        match field {
            1 => name = String::from_utf8_lossy(slice).into_owned(),
            2 => token = String::from_utf8_lossy(slice).into_owned(),
            _ => {}
        }
    }
    if token.is_empty() {
        None
    } else {
        Some((name, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return out;
            }
        }
    }

    fn field(number: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((number << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    /// The exact byte layout DepotDownloader wrote after a QR sign-in.
    fn store(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, token) in entries {
            let mut entry = field(1, name.as_bytes());
            entry.extend(field(2, token.as_bytes()));
            body.extend(field(4, &entry));
        }
        body
    }

    #[test]
    fn reads_a_login_token() {
        let blob = store(&[("zfloams", "AABBCCDD")]);
        let (name, token) = first_token(&blob).unwrap();
        assert_eq!(name, "zfloams");
        assert_eq!(token, "AABBCCDD");
    }

    #[test]
    fn skips_other_fields_and_empty_tokens() {
        // Content-server penalties (field 2) sit before the token map in a real file.
        let mut blob = field(2, &[0x08, 0x05]);
        blob.extend(store(&[("first", "")]));
        blob.extend(store(&[("second", "TOKEN")]));
        let (name, token) = first_token(&blob).unwrap();
        assert_eq!(name, "second");
        assert_eq!(token, "TOKEN");
    }

    #[test]
    fn no_token_means_no_session() {
        assert!(first_token(&store(&[])).is_none());
        // Penalties only: the store exists but nobody is signed in.
        assert!(first_token(&field(2, &[0x08, 0x05])).is_none());
    }

    #[test]
    fn inflates_what_net_deflate_stream_writes() {
        let blob = store(&[("zfloams", "AABBCCDD")]);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&blob).unwrap();
        let packed = encoder.finish().unwrap();
        assert_eq!(inflate(&packed).unwrap(), blob);
    }

    #[test]
    fn garbage_is_not_a_session() {
        assert!(inflate(b"not deflate at all").is_none());
        assert!(first_token(&[]).is_none());
    }

    /// Reads whatever this machine actually has, so the parser is checked against a
    /// file DepotDownloader wrote rather than only against bytes we invented. Skips
    /// when the store is empty, which is the normal state on a fresh machine.
    #[test]
    fn parses_the_store_this_machine_has() {
        let Some(session) = cached_session(&[]) else {
            eprintln!("no cached steam token on this machine");
            return;
        };
        eprintln!("cached steam token: {session:?}");
        assert!(session.signed_in);
        assert!(!session.username.is_empty());
        assert!(!session.path.is_empty());
        assert!(Path::new(&session.path).is_file());
    }
}
