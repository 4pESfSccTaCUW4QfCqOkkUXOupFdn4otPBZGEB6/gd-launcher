//! Background work for the full screen interface.
//!
//! One thread owns one tokio runtime and runs jobs in order. Everything it wants
//! the screen to know comes back as a `Msg` on an unbounded channel, so the UI
//! never blocks on the network, a download or a folder walk.

use crate::config::{self, Config, Instance};
use crate::pull::{Login, PullEvent, PullRequest, Sink};
use crate::{geode, instance, mh, mh_install, paths, pull, steam, util};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum Job {
    /// Rebuild the instance table. Walks folder sizes, so it runs off the UI.
    Scan,
    Install {
        version: String,
        manifest: String,
        name: String,
        /// Mega Hack release picked before the pull, if any.
        mh: Option<String>,
    },
    Launch {
        id: String,
    },
    Remove {
        id: String,
        delete_files: bool,
    },
    MhCatalog {
        version: String,
    },
    MhInstall {
        id: String,
        release: String,
    },
    MhUninstall {
        id: String,
    },
    ModsSearch {
        query: String,
    },
    ModsInstall {
        id: String,
        slug: String,
    },
    ModsRemove {
        id: String,
        slug: String,
    },
    LoaderInstall {
        id: String,
        version: Option<String>,
    },
    Login,
    Import {
        index: usize,
        copy: bool,
    },
}

#[derive(Debug)]
pub enum Msg {
    Log(String),
    Note(String),
    Progress { percent: f64, detail: String },
    Qr(Vec<String>),
    Status(String),
    Error(String),
    Done,
    Quit,
    Reload,
    Instances(Vec<instance::InstanceView>),
    Mods { mods: Vec<geode::ModCard>, total: u64 },
    MhBundles { version: String, bundles: Vec<mh_install::Bundle> },
}

pub struct Worker {
    jobs: UnboundedSender<Job>,
}

impl Worker {
    pub fn spawn(dir: PathBuf, tx: UnboundedSender<Msg>) -> Self {
        let (jobs, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = tx.send(Msg::Error(format!("start runtime: {err}")));
                    return;
                }
            };
            runtime.block_on(async move {
                while let Some(job) = rx.recv().await {
                    run(&dir, job, &tx).await;
                }
            });
        });
        Self { jobs }
    }

    pub fn submit(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

async fn run(dir: &Path, job: Job, tx: &UnboundedSender<Msg>) {
    match job {
        Job::Scan => scan(dir, tx),
        Job::Install {
            version,
            manifest,
            name,
            mh,
        } => finish(install(dir, version, manifest, name, mh, tx).await, tx),
        Job::Launch { id } => finish(launch(dir, &id, tx), tx),
        Job::Remove { id, delete_files } => finish(remove(dir, &id, delete_files, tx), tx),
        Job::MhCatalog { version } => mh_catalog(dir, &version, tx).await,
        Job::MhInstall { id, release } => finish(mh_install_job(dir, &id, &release, tx).await, tx),
        Job::MhUninstall { id } => finish(mh_uninstall_job(dir, &id, tx), tx),
        Job::ModsSearch { query } => mods_search(&query, tx).await,
        Job::ModsInstall { id, slug } => {
            finish(mods_install(dir, &id, &slug, tx).await, tx)
        }
        Job::ModsRemove { id, slug } => finish(mods_remove(dir, &id, &slug, tx), tx),
        Job::LoaderInstall { id, version } => {
            finish(loader_install(dir, &id, version, tx).await, tx)
        }
        Job::Login => finish(login(dir, tx).await, tx),
        Job::Import { index, copy } => finish(import(dir, index, copy, tx), tx),
    }
}

fn finish(result: Result<(), String>, tx: &UnboundedSender<Msg>) {
    if let Err(err) = result {
        let _ = tx.send(Msg::Error(err));
    }
    let _ = tx.send(Msg::Done);
}

fn note(tx: &UnboundedSender<Msg>, text: impl Into<String>) {
    let _ = tx.send(Msg::Note(text.into()));
}

fn status(tx: &UnboundedSender<Msg>, text: impl Into<String>) {
    let _ = tx.send(Msg::Status(text.into()));
}

fn sink_for(tx: UnboundedSender<Msg>) -> Sink {
    Arc::new(move |event| {
        let msg = match event {
            PullEvent::Note(text) => Msg::Note(text),
            PullEvent::Log { line, .. } => Msg::Log(line),
            PullEvent::Qr(lines) => Msg::Qr(lines),
            PullEvent::Progress { percent, detail } => Msg::Progress { percent, detail },
            PullEvent::ToolProgress {
                percent, received, ..
            } => Msg::Progress {
                percent,
                detail: format!("DepotDownloader {}", util::human_bytes(received)),
            },
        };
        let _ = tx.send(msg);
    })
}

fn scan(dir: &Path, tx: &UnboundedSender<Msg>) {
    let cfg = Config::load(dir);
    let rows = cfg
        .instances
        .iter()
        .map(|record| {
            let catalog = cfg
                .version(&record.manifest_id)
                .map(|version| version.version.clone());
            instance::view(record, catalog)
        })
        .collect();
    let _ = tx.send(Msg::Instances(rows));
}

async fn install(
    dir: &Path,
    version: String,
    manifest: String,
    name: String,
    mh_release: Option<String>,
    tx: &UnboundedSender<Msg>,
) -> Result<(), String> {
    let mut cfg = Config::load(dir);
    let root = paths::instances_root(dir, &cfg.settings);
    std::fs::create_dir_all(&root).map_err(|e| format!("create {root:?}: {e}"))?;

    let target = instance::unique_dir(&root, &name, &version);
    std::fs::create_dir_all(&target).map_err(|e| format!("create {target:?}: {e}"))?;

    let record = Instance {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
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

    let _ = tx.send(Msg::Reload);
    note(
        tx,
        format!("instance {} for {version} created", short(&record.id)),
    );

    let login = Login::for_pull(dir, false, false);
    if login == Login::Qr {
        note(tx, "no steam session - a QR code will appear");
    }
    pull::pull(PullRequest {
        dir,
        settings: &cfg.settings,
        target: &target,
        depot_id: &record.depot_id,
        manifest_id: &manifest,
        login,
        attach_stdin: false,
        sink: sink_for(tx.clone()),
    })
    .await?;

    let size = util::dir_size(&target);
    status(
        tx,
        format!(
            "{version}  {}  {} files",
            util::human_bytes(size),
            count_files(&target)
        ),
    );

    if let Some(release) = mh_release {
        let want_geode = geode::loader_present(&target) && !mh::below_2_2(&version);
        match mh_install::install(
            &paths::mh_cache(dir),
            &target,
            &version,
            &release,
            want_geode,
            |progress| {
                let _ = tx.send(Msg::Progress {
                    percent: progress.percent,
                    detail: progress.label,
                });
            },
        )
        .await
        {
            Ok(report) => status(
                tx,
                format!(
                    "mega hack {}  {} files  {}",
                    report.bundle.version,
                    report.files.len(),
                    util::human_bytes(report.bytes)
                ),
            ),
            Err(err) => note(tx, format!("mega hack skipped: {err}")),
        }
    }

    let _ = tx.send(Msg::Reload);
    Ok(())
}

fn launch(dir: &Path, id: &str, tx: &UnboundedSender<Msg>) -> Result<(), String> {
    let cfg = Config::load(dir);

    // An empty id is the Steam tab asking for the copy Steam installed.
    if id.is_empty() {
        let installs = steam::find_installs();
        let install = installs
            .iter()
            .find(|install| steam::looks_like_gd(Path::new(&install.path)))
            .ok_or("no Steam copy found - `gd steam` shows what the launcher can see")?;
        instance::launch_path_with(Path::new(&install.path), true, &cfg.settings.game_args)?;
        status(
            tx,
            format!(
                "launched steam build {}",
                if install.build_id.is_empty() {
                    "?"
                } else {
                    &install.build_id
                }
            ),
        );
        if cfg.settings.close_on_launch {
            let _ = tx.send(Msg::Quit);
        }
        return Ok(());
    }

    let record = find(&cfg, id)?;
    instance::launch_path_with(Path::new(&record.path), true, &cfg.settings.game_args)?;
    status(tx, format!("launched {} {}", short(&record.id), record.version));
    if cfg.settings.close_on_launch {
        let _ = tx.send(Msg::Quit);
    }
    Ok(())
}

fn remove(dir: &Path, id: &str, delete_files: bool, tx: &UnboundedSender<Msg>) -> Result<(), String> {
    let mut cfg = Config::load(dir);
    let index = cfg
        .instances
        .iter()
        .position(|record| record.id == id)
        .ok_or("instance vanished")?;
    let record = cfg.instances.remove(index);
    cfg.save(dir).map_err(|e| format!("save config: {e}"))?;

    if delete_files && !record.linked {
        let freed = util::dir_size(Path::new(&record.path));
        instance::delete_folder(Path::new(&record.path))?;
        status(
            tx,
            format!("removed {}  {} freed", short(&record.id), util::human_bytes(freed)),
        );
    } else if record.linked {
        status(tx, "unregistered the linked Steam copy, files untouched");
    } else {
        status(tx, format!("removed {}, files kept", short(&record.id)));
    }

    let _ = tx.send(Msg::Reload);
    Ok(())
}

async fn mh_catalog(dir: &Path, version: &str, tx: &UnboundedSender<Msg>) {
    let bundles = match mh_install::catalog(&paths::mh_cache(dir)).await {
        Ok(bundles) => bundles,
        Err(err) => {
            note(tx, format!("mega hack catalogue: {err}"));
            Vec::new()
        }
    };
    // Always answer, even empty: an install waiting on this would otherwise hang.
    let _ = tx.send(Msg::MhBundles {
        version: version.to_string(),
        bundles,
    });
    let _ = tx.send(Msg::Done);
}

async fn mh_install_job(
    dir: &Path,
    id: &str,
    release: &str,
    tx: &UnboundedSender<Msg>,
) -> Result<(), String> {
    let cfg = Config::load(dir);
    let record = find(&cfg, id)?;
    let path = PathBuf::from(&record.path);
    let want_geode = geode::loader_present(&path) && !mh::below_2_2(&record.version);

    let report = mh_install::install(
        &paths::mh_cache(dir),
        &path,
        &record.version,
        release,
        want_geode,
        |progress| {
            let _ = tx.send(Msg::Progress {
                percent: progress.percent,
                detail: progress.label,
            });
        },
    )
    .await?;

    status(
        tx,
        format!(
            "mega hack {}  {} files  {}",
            report.bundle.version,
            report.files.len(),
            util::human_bytes(report.bytes)
        ),
    );
    let _ = tx.send(Msg::Reload);
    Ok(())
}

fn mh_uninstall_job(dir: &Path, id: &str, tx: &UnboundedSender<Msg>) -> Result<(), String> {
    let cfg = Config::load(dir);
    let record = find(&cfg, id)?;
    let removed = mh_install::uninstall(Path::new(&record.path))?;
    status(tx, format!("mega hack removed, {removed} files"));
    let _ = tx.send(Msg::Reload);
    Ok(())
}

async fn mods_search(query: &str, tx: &UnboundedSender<Msg>) {
    match geode::search_mods(query.to_string(), 1, 30).await {
        Ok(page) => {
            let _ = tx.send(Msg::Mods {
                mods: page.mods,
                total: page.total,
            });
        }
        Err(err) => note(tx, format!("search: {err}")),
    }
    let _ = tx.send(Msg::Done);
}

async fn mods_install(
    dir: &Path,
    id: &str,
    slug: &str,
    tx: &UnboundedSender<Msg>,
) -> Result<(), String> {
    let cfg = Config::load(dir);
    let record = find(&cfg, id)?;
    let card = geode::mod_detail(slug).await?;
    let name = card.name.clone();
    geode::install_mod(Path::new(&record.path), &card).await?;
    status(tx, format!("installed {name}"));
    let _ = tx.send(Msg::Reload);
    Ok(())
}

fn mods_remove(dir: &Path, id: &str, slug: &str, tx: &UnboundedSender<Msg>) -> Result<(), String> {
    let cfg = Config::load(dir);
    let record = find(&cfg, id)?;
    geode::uninstall_mod(Path::new(&record.path), slug)?;
    status(tx, format!("removed {slug}"));
    let _ = tx.send(Msg::Reload);
    Ok(())
}

async fn loader_install(
    dir: &Path,
    id: &str,
    version: Option<String>,
    tx: &UnboundedSender<Msg>,
) -> Result<(), String> {
    let cfg = Config::load(dir);
    let record = find(&cfg, id)?;
    let version = geode::install_loader(
        Path::new(&record.path),
        version,
        &paths::cache_dir(dir),
        |progress| {
            let _ = tx.send(Msg::Progress {
                percent: progress.percent,
                detail: progress.label,
            });
        },
    )
    .await?;
    status(tx, format!("geode {version} installed"));
    let _ = tx.send(Msg::Reload);
    Ok(())
}

async fn login(dir: &Path, tx: &UnboundedSender<Msg>) -> Result<(), String> {
    let cached = paths::session(dir);
    if cached.signed_in {
        status(tx, format!("already signed in as {}", cached.username));
        return Ok(());
    }

    // DepotDownloader has no login-only mode: `-qr` is answered during a pull.
    // The oldest build is the smallest, so that is the throwaway.
    let smallest = crate::manifests::BUNDLED
        .last()
        .ok_or("bundled manifest table is empty")?;
    let scratch = dir.join("tmp").join("login");
    let cfg = Config::load(dir);

    note(
        tx,
        format!(
            "signing in - pulls Geometry Dash {} once (~40 MB), then deletes it",
            smallest.version
        ),
    );
    pull::pull(PullRequest {
        dir,
        settings: &cfg.settings,
        target: &scratch,
        depot_id: config::GD_WIN_DEPOT,
        manifest_id: smallest.manifest,
        login: Login::Qr,
        attach_stdin: false,
        sink: sink_for(tx.clone()),
    })
    .await?;
    let _ = std::fs::remove_dir_all(&scratch);

    let after = paths::session(dir);
    if after.signed_in {
        status(tx, format!("signed in {}", after.username));
    } else {
        note(tx, "pull finished but no token was written - check the Steam app");
    }
    let _ = tx.send(Msg::Reload);
    Ok(())
}

fn import(dir: &Path, index: usize, copy: bool, tx: &UnboundedSender<Msg>) -> Result<(), String> {
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
    let label = "Steam copy".to_string();

    let (path, linked) = if copy {
        let target = instance::unique_dir(&root, &label, &install.build_id);
        note(tx, format!("copying {} - this takes a minute", source.display()));
        instance::copy_tree(&source, &target, |progress| {
            let percent = progress
                .get("percent")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let label = progress
                .get("label")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let _ = tx.send(Msg::Progress {
                percent,
                detail: label,
            });
        })?;
        (target, false)
    } else {
        (source.clone(), true)
    };

    let version = cfg
        .version(&install.manifest_id)
        .map(|entry| entry.version.clone())
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

    status(
        tx,
        format!(
            "imported {}{}",
            record.version,
            if linked { "  (linked)" } else { "" }
        ),
    );
    let _ = tx.send(Msg::Reload);
    Ok(())
}

fn find<'a>(cfg: &'a Config, id: &str) -> Result<&'a Instance, String> {
    cfg.instances
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| "instance vanished - press r".to_string())
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .count()
}
