//! Shared plumbing: one HTTP client, verified downloads, zip extraction, folder sizes.

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const USER_AGENT: &str = "GDLauncher/1.0 (+https://geode-sdk.org)";

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub label: String,
    pub received: u64,
    pub total: u64,
    pub percent: f64,
}

/// Stream `url` into `dest`, reporting progress through `on_progress`. Returns
/// the byte count. No Tauri types, so the CLI shares this exact path.
pub async fn download(
    label: &str,
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<u64, String> {
    let client = http_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GET {url}: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }

    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("create {tmp:?}: {e}"))?;
    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream {url}: {e}"))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write {tmp:?}: {e}"))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(120) {
            last_emit = std::time::Instant::now();
            let percent = if total > 0 {
                (received as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            on_progress(DownloadProgress {
                label: label.to_string(),
                received,
                total,
                percent,
            });
        }
    }
    use tokio::io::AsyncWriteExt;
    file.flush().await.map_err(|e| format!("flush {tmp:?}: {e}"))?;
    drop(file);

    // Only replace the real file once the whole body landed.
    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename {tmp:?} -> {dest:?}: {e}"))?;

    on_progress(DownloadProgress {
        label: label.to_string(),
        received,
        total: if total == 0 { received } else { total },
        percent: 100.0,
    });
    Ok(received)
}

/// Stream `url` into `dest`, emitting progress on `event`. Returns the byte count.
pub async fn download_to_file(
    app: &AppHandle,
    event: &str,
    label: &str,
    url: &str,
    dest: &Path,
) -> Result<u64, String> {
    download(label, url, dest, |progress| {
        let _ = app.emit(event, progress);
    })
    .await
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("read {path:?}: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Extract every entry of `zip_path` into `dest`, overwriting. Used for the
/// Geode loader bundle and DepotDownloader, both of which are flat archives.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open {zip_path:?}: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip {zip_path:?}: {e}"))?;
    std::fs::create_dir_all(dest).map_err(|e| format!("mkdir {dest:?}: {e}"))?;
    let mut written = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // rejects ../ escapes
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("mkdir {out_path:?}: {e}"))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("create {out_path:?}: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract {out_path:?}: {e}"))?;
        written += 1;
    }
    Ok(written)
}

/// Total bytes under `path`, skipping symlinks so a loop cannot hang the walk.
pub fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Strip characters Windows rejects in file names.
pub fn sanitize_component(name: &str) -> String {
    const BAD: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = name
        .chars()
        .map(|c| if BAD.contains(&c) || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "instance".into()
    } else {
        trimmed
    }
}
