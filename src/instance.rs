//! Instance lifecycle: create from a manifest, import an existing Steam copy,
//! inspect, launch, remove.

use crate::config::{self, Instance};
use crate::util;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct InstanceView {
    #[serde(flatten)]
    pub base: Instance,
    pub exists: bool,
    pub has_exe: bool,
    pub geode: bool,
    pub mod_count: usize,
    pub size_bytes: u64,
    pub size_human: String,
    /// Version label the manifest catalogue knows about, if any.
    pub catalog_version: Option<String>,
}

pub fn view(instance: &Instance, catalog_version: Option<String>) -> InstanceView {
    let path = PathBuf::from(&instance.path);
    let exists = path.exists();
    let has_exe = path.join("GeometryDash.exe").exists();
    let geode = crate::geode::loader_present(&path);
    let mod_count = if geode {
        crate::geode::list_installed(&path).len()
    } else {
        0
    };
    let size_bytes = if exists { util::dir_size(&path) } else { 0 };
    InstanceView {
        base: instance.clone(),
        exists,
        has_exe,
        geode,
        mod_count,
        size_bytes,
        size_human: util::human_bytes(size_bytes),
        catalog_version,
    }
}

/// Folder name for a new instance: `<slug>-<version>` plus a counter if taken.
pub fn unique_dir(root: &Path, name: &str, version: &str) -> PathBuf {
    let base = format!(
        "{}-{}",
        config::slug(name),
        config::slug(if version.is_empty() { "build" } else { version })
    );
    let mut candidate = root.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = root.join(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

/// Copy a Steam install into a managed folder. Progress is handed to the caller
/// per top-level entry, so a terminal can draw a line while 336 MB moves.
pub fn copy_tree(
    from: &Path,
    to: &Path,
    mut on_progress: impl FnMut(serde_json::Value),
) -> Result<u64, String> {
    let entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|e| format!("read {from:?}: {e}"))?
        .filter_map(|e| e.ok())
        .collect();
    let total = entries.len().max(1);
    let mut copied = 0u64;

    std::fs::create_dir_all(to).map_err(|e| format!("create {to:?}: {e}"))?;
    for (i, entry) in entries.iter().enumerate() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().to_string();
        // The launcher's own per-instance metadata must not travel with the copy.
        if name == ".gdlauncher" {
            continue;
        }
        copy_entry(&src, &dst)?;
        copied += 1;
        on_progress(serde_json::json!({
            "label": name,
            "received": i + 1,
            "done": i + 1,
            "total": total,
            "percent": ((i + 1) as f64 / total as f64) * 100.0,
        }));
    }
    Ok(copied)
}

fn copy_entry(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| format!("stat {src:?}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Ok(()); // never follow links out of the source tree
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
        for entry in std::fs::read_dir(src)
            .map_err(|e| format!("read {src:?}: {e}"))?
            .flatten()
        {
            copy_entry(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        std::fs::copy(src, dst).map_err(|e| format!("copy {src:?} -> {dst:?}: {e}"))?;
    }
    Ok(())
}

/// Write the per-instance marker the launcher uses to recognise its own folders.
pub fn write_marker(path: &Path, instance: &Instance) -> Result<(), String> {
    let dir = path.join(".gdlauncher");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {dir:?}: {e}"))?;
    let body = serde_json::to_vec_pretty(instance).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("instance.json"), body)
        .map_err(|e| format!("write instance marker: {e}"))
}

/// Start the game. `steam_appid.txt` is what makes steam_api bind to 322170 even
/// when the exe is launched outside the Steam client.
pub fn launch(instance: &Instance) -> Result<(), String> {
    launch_path(Path::new(&instance.path), false)
}

/// Start `GeometryDash.exe` from any folder. Split out so a Steam install that
/// is not a managed instance can be launched straight from its own directory.
///
/// `quiet` drops the child's stdio, which matters in a terminal: Geode prints its
/// whole loader log to stdout and none of it belongs in a one-line launch.
pub fn launch_path(path: &Path, quiet: bool) -> Result<(), String> {
    launch_path_with(path, quiet, "")
}

/// Same launch, with extra arguments appended. `args` is split on whitespace;
/// there is no quoting layer, so a path with spaces cannot go through here.
pub fn launch_path_with(path: &Path, quiet: bool, args: &str) -> Result<(), String> {
    let exe = path.join("GeometryDash.exe");
    if !exe.exists() {
        return Err("GeometryDash.exe is missing from this instance".into());
    }
    let _ = std::fs::write(path.join("steam_appid.txt"), config::GD_APP_ID);

    let mut cmd = std::process::Command::new(&exe);
    cmd.current_dir(path);
    for arg in args.split_whitespace() {
        cmd.arg(arg);
    }
    if quiet {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 0x08000000 = CREATE_NO_WINDOW, keeps the launcher's console hidden.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    Ok(())
}

/// Delete an instance folder. Only called for launcher-owned directories -
/// linked Steam installs are unregistered, never deleted.
pub fn delete_folder(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    if name.is_empty() || name == "common" || name == "steamapps" {
        return Err("refusing to delete a Steam library folder".into());
    }
    std::fs::remove_dir_all(path).map_err(|e| format!("delete {path:?}: {e}"))
}
