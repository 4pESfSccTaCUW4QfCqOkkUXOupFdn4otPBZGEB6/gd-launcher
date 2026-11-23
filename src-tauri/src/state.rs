//! Process-global mutable state.

use crate::config::Config;
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::Mutex;

pub struct RunningJob {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    /// True when this run authenticated with a QR code, so a clean exit can mark
    /// the session as established.
    pub used_qr: bool,
}

pub struct AppState {
    pub config: Mutex<Config>,
    pub data_dir: PathBuf,
    /// Only one depot download at a time - two DepotDownloader processes writing
    /// into the same Steam CDN cache is how you get half-written chunks.
    pub job: Mutex<Option<RunningJob>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let config = Config::load(&data_dir);
        Self {
            config: Mutex::new(config),
            data_dir,
            job: Mutex::new(None),
        }
    }

    pub fn persist(&self) -> Result<(), String> {
        let cfg = self.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.save(&self.data_dir)
            .map_err(|e| format!("save config: {e}"))
    }

    pub fn snapshot(&self) -> Result<Config, String> {
        self.config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| "config lock poisoned".to_string())
    }
}
