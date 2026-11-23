//! Full screen interface.
//!
//! Six tabs over the same data the command line uses. Every slow thing (a pull,
//! a catalogue fetch, a folder walk) goes to the worker thread in `jobs`, so the
//! screen keeps drawing while a 3 GB instance downloads.

mod jobs;
mod settings;
mod theme;
mod view;

use crate::config::{Config, Settings};
use crate::{geode, instance, manifests, mh, mh_install, paths, steam};
use jobs::{Job, Msg, Worker};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use settings::{Kind, Setting};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Instances,
    Versions,
    Mods,
    MegaHack,
    Steam,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Instances,
        Tab::Versions,
        Tab::Mods,
        Tab::MegaHack,
        Tab::Steam,
        Tab::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Instances => "Instances",
            Tab::Versions => "Versions",
            Tab::Mods => "Mods",
            Tab::MegaHack => "Mega Hack",
            Tab::Steam => "Steam",
            Tab::Settings => "Settings",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    pub fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }

    fn from_digit(digit: char) -> Option<Tab> {
        let index = digit.to_digit(10)? as usize;
        if index == 0 || index > Tab::ALL.len() {
            return None;
        }
        Some(Tab::ALL[index - 1])
    }
}

#[derive(Default)]
pub struct ModsState {
    pub for_id: String,
    pub installed: Vec<geode::InstalledMod>,
    pub results: Vec<geode::ModCard>,
    pub total: u64,
    pub searching: bool,
    pub sel: usize,
}

#[derive(Default)]
pub struct MhState {
    pub for_id: String,
    pub version: String,
    pub bundles: Vec<mh_install::Bundle>,
    pub sel: usize,
}

#[derive(Default)]
pub struct SteamState {
    pub installs: Vec<steam::SteamInstall>,
    pub accounts: Vec<steam::SteamAccount>,
    pub sel: usize,
}

pub struct JobView {
    pub label: String,
    pub percent: Option<f64>,
    pub detail: String,
}

enum Overlay {
    Help,
    Confirm(Confirm),
    Input(Input),
}

struct Confirm {
    title: String,
    body: String,
    yes_label: String,
    no_label: String,
    yes: Action,
    no: Option<Action>,
}

struct Input {
    title: String,
    prompt: String,
    value: String,
    action: InputAction,
}

#[derive(Clone)]
enum InputAction {
    InstallName {
        version: String,
        manifest: String,
    },
    ModsSearch,
    SettingText(Setting),
}

#[derive(Clone)]
enum Action {
    Quit,
    Install {
        version: String,
        manifest: String,
        name: String,
        mh: Option<String>,
    },
    Remove {
        id: String,
        delete_files: bool,
    },
    MhUninstall {
        id: String,
    },
    Loader {
        id: String,
    },
    ModsRemove {
        id: String,
        slug: String,
    },
    ResetSettings,
}

pub struct App {
    dir: PathBuf,
    cfg: Config,
    theme: &'static theme::Theme,
    tab: Tab,
    quit: bool,
    overlay: Option<Overlay>,

    instances: Vec<instance::InstanceView>,
    instance_sel: usize,
    version_sel: usize,
    mods: ModsState,
    mh: MhState,
    steam: SteamState,
    settings_sel: usize,

    log: VecDeque<String>,
    status: String,
    show_log: bool,
    job: Option<JobView>,
    pending: usize,
    qr: Option<Vec<String>>,
    frames: u64,
    session: String,
    licence: mh_install::LicenseStatus,
    mh_installed: Option<(String, usize)>,
    mh_loading: bool,
    /// Set while an install waits on the Mega Hack catalogue.
    pending_install: Option<PendingInstall>,

    worker: Worker,
    rx: UnboundedReceiver<Msg>,
}

struct PendingInstall {
    version: String,
    manifest: String,
    name: String,
}

pub fn run(dir: &Path) -> Result<(), String> {
    let mut terminal = ratatui::try_init().map_err(|e| format!("terminal: {e}"))?;
    let mut app = App::new(dir);
    let result = app.main_loop(&mut terminal);
    ratatui::restore();
    result
}

impl App {
    fn new(dir: &Path) -> Self {
        let cfg = Config::load(dir);
        let theme = theme::by_name(&cfg.settings.tui_theme);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = Worker::spawn(dir.to_path_buf(), tx);
        worker.submit(Job::Scan);

        let mut app = Self {
            dir: dir.to_path_buf(),
            cfg,
            theme,
            tab: Tab::Instances,
            quit: false,
            overlay: None,
            instances: Vec::new(),
            instance_sel: 0,
            version_sel: 0,
            mods: ModsState::default(),
            mh: MhState::default(),
            steam: SteamState::default(),
            settings_sel: 0,
            log: VecDeque::new(),
            status: "ready".into(),
            show_log: true,
            job: None,
            pending: 1,
            qr: None,
            frames: 0,
            session: session_label(dir),
            licence: mh_install::license_status(),
            mh_installed: None,
            mh_loading: false,
            pending_install: None,
            worker,
            rx,
        };
        app.push_log(format!(
            "gd {}  {} instances  {}",
            env!("CARGO_PKG_VERSION"),
            app.cfg.instances.len(),
            app.cfg.settings.tui_theme
        ));
        app
    }

    fn main_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<(), String> {
        loop {
            while let Ok(msg) = self.rx.try_recv() {
                self.handle(msg);
            }
            terminal
                .draw(|frame| view::draw(frame, self))
                .map_err(|e| format!("draw: {e}"))?;

            if event::poll(Duration::from_millis(120)).map_err(|e| format!("input: {e}"))? {
                if let Event::Key(key) = event::read().map_err(|e| format!("input: {e}"))? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
            self.frames = self.frames.wrapping_add(1);
            if self.quit {
                break;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- messages

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Log(line) => self.push_log(line),
            Msg::Note(text) => {
                self.push_log(text.clone());
                self.status = text;
            }
            Msg::Progress { percent, detail } => {
                if let Some(job) = self.job.as_mut() {
                    job.percent = Some(percent);
                    job.detail = detail;
                }
            }
            Msg::Qr(lines) => {
                self.qr = Some(lines);
                self.push_log("scan the code with the Steam mobile app".into());
            }
            Msg::Status(text) => {
                self.status = text.clone();
                self.push_log(text);
            }
            Msg::Error(err) => {
                self.status = format!("error: {err}");
                self.push_log(format!("error: {err}"));
            }
            Msg::Done => {
                self.pending = self.pending.saturating_sub(1);
                if self.pending == 0 {
                    self.job = None;
                    self.qr = None;
                }
            }
            Msg::Quit => self.quit = true,
            Msg::Reload => {
                self.cfg = Config::load(&self.dir);
                self.theme = theme::by_name(&self.cfg.settings.tui_theme);
                self.session = session_label(&self.dir);
                self.licence = mh_install::license_status();
                self.internal(Job::Scan);
                match self.tab {
                    Tab::Mods => self.refresh_mods(),
                    Tab::MegaHack => self.load_mh_catalog(),
                    Tab::Steam => self.refresh_steam(),
                    _ => {}
                }
            }
            Msg::Instances(rows) => {
                self.instances = rows;
                self.clamp_all();
                match self.tab {
                    Tab::Mods => self.refresh_mods(),
                    Tab::MegaHack => {
                        self.mh_installed = self
                            .selected_instance()
                            .and_then(|view| mh_install::installed(Path::new(&view.base.path)))
                            .map(|installed| (installed.release, installed.files.len()));
                    }
                    _ => {}
                }
            }
            Msg::Mods { mods, total } => {
                self.mods.results = mods;
                self.mods.total = total;
                self.mods.searching = true;
                self.mods.sel = 0;
                self.status = format!("{} results", self.mods.total);
            }
            Msg::MhBundles { version, bundles } => {
                self.mh.version = version.clone();
                self.mh.bundles = bundles.clone();
                self.mh_loading = false;
                let waiting = self
                    .pending_install
                    .as_ref()
                    .map(|pending| pending.version == version)
                    .unwrap_or(false);
                if waiting {
                    let pending = self.pending_install.take().expect("checked");
                    self.offer_mega_hack(pending, &bundles);
                }
            }
        }
    }

    fn push_log(&mut self, line: String) {
        if line.trim().is_empty() {
            return;
        }
        if self.log.len() >= 200 {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    // -------------------------------------------------------------- submits

    fn submit(&mut self, label: impl Into<String>, job: Job) {
        self.pending += 1;
        if self.job.is_none() {
            self.job = Some(JobView {
                label: label.into(),
                percent: None,
                detail: String::new(),
            });
        }
        self.worker.submit(job);
    }

    /// Work the screen asked for, not the operator. It still counts as pending so
    /// quitting mid-scan is handled, but it never replaces the job label.
    fn internal(&mut self, job: Job) {
        self.pending += 1;
        self.worker.submit(job);
    }

    // ----------------------------------------------------------------- keys

    fn on_key(&mut self, key: KeyEvent) {
        if self.overlay.is_some() {
            let overlay = self.overlay.take().expect("checked");
            self.overlay = self.overlay_key(overlay, key);
            return;
        }

        // The QR is an overlay in spirit but it is not modal: the pull is still
        // running underneath and esc only hides the code.
        if self.qr.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.qr = None;
                return;
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.request_quit()
            }
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Tab => self.set_tab(self.tab.next()),
            KeyCode::BackTab => self.set_tab(self.tab.prev()),
            KeyCode::Char(digit @ '1'..='6') => {
                if let Some(tab) = Tab::from_digit(digit) {
                    self.set_tab(tab);
                }
            }
            KeyCode::Char('L') => self.show_log = !self.show_log,
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Home | KeyCode::Char('g') => self.jump(false),
            KeyCode::End | KeyCode::Char('G') => self.jump(true),
            KeyCode::Esc => self.escape(),
            _ => self.tab_key(key),
        }
    }

    fn overlay_key(&mut self, overlay: Overlay, key: KeyEvent) -> Option<Overlay> {
        match overlay {
            Overlay::Help => None,
            Overlay::Confirm(confirm) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.run_action(confirm.yes);
                    None
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if let Some(no) = confirm.no {
                        self.run_action(no);
                    }
                    None
                }
                KeyCode::Esc => None,
                _ => Some(Overlay::Confirm(confirm)),
            },
            Overlay::Input(mut input) => match key.code {
                KeyCode::Esc => None,
                KeyCode::Enter => {
                    self.submit_input(input);
                    None
                }
                KeyCode::Backspace => {
                    input.value.pop();
                    Some(Overlay::Input(input))
                }
                KeyCode::Char(ch) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        input.value.push(ch);
                    }
                    Some(Overlay::Input(input))
                }
                _ => Some(Overlay::Input(input)),
            },
        }
    }

    fn submit_input(&mut self, input: Input) {
        match input.action {
            InputAction::InstallName { version, manifest } => {
                self.after_name(version, manifest, input.value)
            }
            InputAction::ModsSearch => {
                let query = input.value.trim().to_string();
                if query.is_empty() {
                    return;
                }
                self.submit(format!("search {query}"), Job::ModsSearch { query });
            }
            InputAction::SettingText(setting) => {
                if setting.set_text(&mut self.cfg.settings, &input.value) {
                    self.save_settings();
                }
            }
        }
    }

    fn request_quit(&mut self) {
        if self.pending > 0 {
            self.overlay = Some(Overlay::Confirm(Confirm {
                title: "quit".into(),
                body: format!(
                    "{} job(s) still running. The worker stops when this screen closes.",
                    self.pending
                ),
                yes_label: "quit".into(),
                no_label: "stay".into(),
                yes: Action::Quit,
                no: None,
            }));
        } else {
            self.quit = true;
        }
    }

    fn escape(&mut self) {
        if self.tab == Tab::Mods && self.mods.searching {
            self.mods.searching = false;
            self.mods.results.clear();
            self.mods.sel = 0;
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        match tab {
            Tab::Mods => self.refresh_mods(),
            Tab::MegaHack => self.load_mh_catalog(),
            Tab::Steam => self.refresh_steam(),
            _ => {}
        }
    }

    fn tab_key(&mut self, key: KeyEvent) {
        match self.tab {
            Tab::Instances => self.instances_key(key),
            Tab::Versions => self.versions_key(key),
            Tab::Mods => self.mods_key(key),
            Tab::MegaHack => self.mh_key(key),
            Tab::Steam => self.steam_key(key),
            Tab::Settings => self.settings_key(key),
        }
    }

    fn instances_key(&mut self, key: KeyEvent) {
        let Some(view) = self.instances.get(self.instance_sel) else {
            if matches!(key.code, KeyCode::Char('i')) {
                self.set_tab(Tab::Versions);
            }
            return;
        };
        let id = view.base.id.clone();
        let name = view.base.name.clone();
        let version = view.base.version.clone();
        let path = view.base.path.clone();

        match key.code {
            KeyCode::Enter => self.submit("launch", Job::Launch { id }),
            KeyCode::Char('i') => self.set_tab(Tab::Versions),
            KeyCode::Char('m') => self.set_tab(Tab::MegaHack),
            KeyCode::Char('r') => self.internal(Job::Scan),
            KeyCode::Char('o') => {
                let path = PathBuf::from(&path);
                if path.exists() {
                    let _ = std::process::Command::new("explorer").arg(&path).spawn();
                } else {
                    self.status = format!("{} is missing", path.display());
                }
            }
            KeyCode::Char('l') => {
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "geode loader".into(),
                    body: format!("Install the newest stable Geode loader over {name} {version}?"),
                    yes_label: "install".into(),
                    no_label: "cancel".into(),
                    yes: Action::Loader { id },
                    no: None,
                }));
            }
            KeyCode::Char('d') => {
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "remove instance".into(),
                    body: format!(
                        "{name}  {version}\n\nDelete the folder and its files, or only unregister it?"
                    ),
                    yes_label: "delete files".into(),
                    no_label: "keep files".into(),
                    yes: Action::Remove {
                        id: id.clone(),
                        delete_files: true,
                    },
                    no: Some(Action::Remove {
                        id,
                        delete_files: false,
                    }),
                }));
            }
            _ => {}
        }
    }

    fn versions_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Enter {
            if let Some(entry) = manifests::BUNDLED.get(self.version_sel) {
                self.overlay = Some(Overlay::Input(Input {
                    title: "install".into(),
                    prompt: format!("name the instance for {}", entry.version),
                    value: format!("Geometry Dash {}", entry.version),
                    action: InputAction::InstallName {
                        version: entry.version.to_string(),
                        manifest: entry.manifest.to_string(),
                    },
                }));
            }
        }
    }

    fn mods_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s') => {
                self.overlay = Some(Overlay::Input(Input {
                    title: "mod search".into(),
                    prompt: "query".into(),
                    value: String::new(),
                    action: InputAction::ModsSearch,
                }));
            }
            KeyCode::Enter => {
                if self.mods.searching {
                    let Some(card) = self.mods.results.get(self.mods.sel) else {
                        return;
                    };
                    let Some(view) = self.instances.get(self.instance_sel) else {
                        return;
                    };
                    let slug = card.id.clone();
                    let id = view.base.id.clone();
                    self.submit(format!("install {slug}"), Job::ModsInstall { id, slug });
                }
            }
            KeyCode::Char('x') => {
                if self.mods.searching {
                    return;
                }
                let Some(module) = self.mods.installed.get(self.mods.sel) else {
                    return;
                };
                let Some(view) = self.instances.get(self.instance_sel) else {
                    return;
                };
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "remove mod".into(),
                    body: format!("Remove {} {}?", module.name, module.version),
                    yes_label: "remove".into(),
                    no_label: "cancel".into(),
                    yes: Action::ModsRemove {
                        id: view.base.id.clone(),
                        slug: module.id.clone(),
                    },
                    no: None,
                }));
            }
            _ => {}
        }
    }

    fn mh_key(&mut self, key: KeyEvent) {
        let Some(view) = self.instances.get(self.instance_sel) else {
            return;
        };
        let version = view.base.version.clone();
        let id = view.base.id.clone();

        match key.code {
            KeyCode::Enter => {
                let options = mh::options_for(&version);
                let Some(release) = options.get(self.mh.sel) else {
                    return;
                };
                let release = release.version.clone();
                self.submit(
                    format!("mega hack {release}"),
                    Job::MhInstall {
                        id,
                        release: release.clone(),
                    },
                );
            }
            KeyCode::Char('x') => {
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "remove mega hack".into(),
                    body: format!("Remove Mega Hack from {version}? Only files it installed are deleted."),
                    yes_label: "remove".into(),
                    no_label: "cancel".into(),
                    yes: Action::MhUninstall { id },
                    no: None,
                }));
            }
            KeyCode::Char('c') => self.load_mh_catalog(),
            _ => {}
        }
    }

    fn steam_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.submit("launch steam", Job::Launch { id: String::new() }),
            KeyCode::Char('c') => {
                if self.steam.installs.get(self.steam.sel).is_some() {
                    self.submit(
                        "import steam copy",
                        Job::Import {
                            index: self.steam.sel,
                            copy: true,
                        },
                    );
                }
            }
            KeyCode::Char('l') => {
                if self.steam.installs.get(self.steam.sel).is_some() {
                    self.submit(
                        "link steam copy",
                        Job::Import {
                            index: self.steam.sel,
                            copy: false,
                        },
                    );
                }
            }
            KeyCode::Char('s') => self.submit("steam sign in", Job::Login),
            KeyCode::Char('r') => self.refresh_steam(),
            _ => {}
        }
    }

    fn settings_key(&mut self, key: KeyEvent) {
        let setting = Setting::ALL[self.settings_sel];
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if setting.step(&mut self.cfg.settings, -1) {
                    self.save_settings();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if setting.step(&mut self.cfg.settings, 1) {
                    self.save_settings();
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if setting.kind() == Kind::Text {
                    let value = setting.edit_text(&self.cfg.settings).unwrap_or_default();
                    self.overlay = Some(Overlay::Input(Input {
                        title: "settings".into(),
                        prompt: setting.label().into(),
                        value,
                        action: InputAction::SettingText(setting),
                    }));
                } else if setting.step(&mut self.cfg.settings, 1) {
                    self.save_settings();
                }
            }
            KeyCode::Char('R') => {
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "reset settings".into(),
                    body: "Put every setting back to its default?\nInstances are not touched.".into(),
                    yes_label: "reset".into(),
                    no_label: "cancel".into(),
                    yes: Action::ResetSettings,
                    no: None,
                }));
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------- actions

    fn after_name(&mut self, version: String, manifest: String, value: String) {
        let name = if value.trim().is_empty() {
            format!("Geometry Dash {version}")
        } else {
            value.trim().to_string()
        };

        // Below 2.2 the catalogue decides whether there is anything to offer, so
        // the question is only asked when a package actually exists.
        if mh::below_2_2(&version) {
            self.status = format!("checking the Mega Hack catalogue for {version}");
            self.pending_install = Some(PendingInstall {
                version: version.clone(),
                manifest,
                name,
            });
            self.internal(Job::MhCatalog { version });
            return;
        }

        self.start_install(version, manifest, name, None);
    }

    fn offer_mega_hack(&mut self, pending: PendingInstall, bundles: &[mh_install::Bundle]) {
        let usable = bundles.iter().find(|bundle| {
            bundle.gjver == pending.version
                && !bundle.geode
                && (!bundle.paid() || self.licence.signed_in)
        });

        let Some(bundle) = usable else {
            self.status = format!(
                "no installable Mega Hack package for {}, pulling the build only",
                pending.version
            );
            self.start_install(pending.version, pending.manifest, pending.name, None);
            return;
        };

        let release = bundle.version.clone();
        self.overlay = Some(Overlay::Confirm(Confirm {
            title: "mega hack".into(),
            body: format!(
                "Install Mega Hack {} for {}?\n\nThe depot pull runs either way.",
                release, pending.version
            ),
            yes_label: "install".into(),
            no_label: "skip".into(),
            yes: Action::Install {
                version: pending.version.clone(),
                manifest: pending.manifest.clone(),
                name: pending.name.clone(),
                mh: Some(release),
            },
            no: Some(Action::Install {
                version: pending.version,
                manifest: pending.manifest,
                name: pending.name,
                mh: None,
            }),
        }));
    }

    fn start_install(&mut self, version: String, manifest: String, name: String, mh: Option<String>) {
        self.submit(
            format!("install {version}"),
            Job::Install {
                version,
                manifest,
                name,
                mh,
            },
        );
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Install {
                version,
                manifest,
                name,
                mh,
            } => self.start_install(version, manifest, name, mh),
            Action::Remove { id, delete_files } => {
                self.submit("remove", Job::Remove { id, delete_files })
            }
            Action::MhUninstall { id } => {
                self.submit("mega hack remove", Job::MhUninstall { id })
            }
            Action::Loader { id } => self.submit(
                "geode loader",
                Job::LoaderInstall {
                    id,
                    version: None,
                },
            ),
            Action::ModsRemove { id, slug } => {
                self.submit("mod remove", Job::ModsRemove { id, slug })
            }
            Action::ResetSettings => {
                self.cfg.settings = Settings::default();
                self.save_settings();
                self.status = "settings reset".into();
            }
        }
    }

    fn save_settings(&mut self) {
        self.theme = theme::by_name(&self.cfg.settings.tui_theme);
        if let Err(err) = self.cfg.save(&self.dir) {
            self.status = format!("save config: {err}");
        }
    }

    // --------------------------------------------------------------- state

    fn refresh_mods(&mut self) {
        let Some(view) = self.instances.get(self.instance_sel) else {
            self.mods.installed.clear();
            self.mods.for_id.clear();
            return;
        };
        let same = self.mods.for_id == view.base.id;
        self.mods.for_id = view.base.id.clone();
        self.mods.installed = geode::list_installed(Path::new(&view.base.path));
        if !same {
            self.mods.results.clear();
            self.mods.searching = false;
            self.mods.sel = 0;
        }
        self.mods.sel = self
            .mods
            .sel
            .min(self.mods.installed.len().saturating_sub(1));
    }

    fn load_mh_catalog(&mut self) {
        let Some(view) = self.instances.get(self.instance_sel) else {
            return;
        };
        let version = view.base.version.clone();
        self.mh.for_id = view.base.id.clone();
        self.mh.version = version.clone();
        self.mh.sel = self.mh.sel.min(mh::options_for(&version).len().saturating_sub(1));
        self.licence = mh_install::license_status();
        self.mh_installed = mh_install::installed(Path::new(&view.base.path))
            .map(|installed| (installed.release, installed.files.len()));
        self.mh_loading = true;
        self.internal(Job::MhCatalog { version });
    }

    fn refresh_steam(&mut self) {
        self.steam.installs = steam::find_installs();
        self.steam.accounts = steam::steam_accounts();
        self.steam.sel = self.steam.sel.min(self.steam.installs.len().saturating_sub(1));
    }

    fn clamp_all(&mut self) {
        let instances = self.instances.len();
        self.instance_sel = self.instance_sel.min(instances.saturating_sub(1));
        self.version_sel = self
            .version_sel
            .min(manifests::BUNDLED.len().saturating_sub(1));
        self.settings_sel = self.settings_sel.min(Setting::ALL.len() - 1);
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.len();
        if len == 0 {
            *self.sel_mut() = 0;
            return;
        }
        let sel = self.sel_mut();
        if *sel >= len {
            *sel = len - 1;
        }
    }

    fn len(&self) -> usize {
        match self.tab {
            Tab::Instances => self.instances.len(),
            Tab::Versions => manifests::BUNDLED.len(),
            Tab::Mods => {
                if self.mods.searching {
                    self.mods.results.len()
                } else {
                    self.mods.installed.len()
                }
            }
            Tab::MegaHack => {
                let version = self
                    .instances
                    .get(self.instance_sel)
                    .map(|view| view.base.version.as_str())
                    .unwrap_or("");
                mh::options_for(version).len()
            }
            Tab::Steam => self.steam.installs.len(),
            Tab::Settings => Setting::ALL.len(),
        }
    }

    fn sel_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Instances => &mut self.instance_sel,
            Tab::Versions => &mut self.version_sel,
            Tab::Mods => &mut self.mods.sel,
            Tab::MegaHack => &mut self.mh.sel,
            Tab::Steam => &mut self.steam.sel,
            Tab::Settings => &mut self.settings_sel,
        }
    }

    fn move_sel(&mut self, delta: i32) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let sel = self.sel_mut();
        let next = (*sel as i32 + delta).clamp(0, len as i32 - 1);
        *sel = next as usize;
    }

    fn jump(&mut self, end: bool) {
        let len = self.len();
        if len == 0 {
            return;
        }
        *self.sel_mut() = if end { len - 1 } else { 0 };
    }

    fn selected_instance(&self) -> Option<&instance::InstanceView> {
        self.instances.get(self.instance_sel)
    }
}

fn session_label(dir: &Path) -> String {
    let cached = paths::session(dir);
    if cached.signed_in {
        format!("steam {}", cached.username)
    } else {
        "no steam session".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn scratch() -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!("gd-tui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (App::new(&dir), dir)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn setting_index(setting: Setting) -> usize {
        Setting::ALL
            .iter()
            .position(|candidate| *candidate == setting)
            .unwrap()
    }

    #[test]
    fn every_tab_draws_at_a_normal_size() {
        let (mut app, dir) = scratch();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        for tab in Tab::ALL {
            app.set_tab(tab);
            terminal
                .draw(|frame| view::draw(frame, &app))
                .expect("draw");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_tab_draws_in_a_small_window() {
        let (mut app, dir) = scratch();
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        for tab in Tab::ALL {
            app.set_tab(tab);
            terminal
                .draw(|frame| view::draw(frame, &app))
                .expect("draw");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn numbers_switch_tabs() {
        let (mut app, dir) = scratch();
        app.on_key(key(KeyCode::Char('2')));
        assert_eq!(app.tab, Tab::Versions);
        app.on_key(key(KeyCode::Char('6')));
        assert_eq!(app.tab, Tab::Settings);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Instances);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_theme_step_lands_in_the_config_file() {
        let (mut app, dir) = scratch();
        app.set_tab(Tab::Settings);
        app.settings_sel = setting_index(Setting::Theme);
        app.on_key(key(KeyCode::Right));
        assert_ne!(app.cfg.settings.tui_theme, "amber");
        let saved = Config::load(&dir);
        assert_eq!(saved.settings.tui_theme, app.cfg.settings.tui_theme);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_tui_toggle_persists() {
        let (mut app, dir) = scratch();
        app.set_tab(Tab::Settings);
        app.settings_sel = setting_index(Setting::Tui);
        let before = app.cfg.settings.tui;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.cfg.settings.tui, !before);
        assert_eq!(Config::load(&dir).settings.tui, !before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn install_opens_a_name_prompt_and_esc_closes_it() {
        let (mut app, dir) = scratch();
        app.set_tab(Tab::Versions);
        app.on_key(key(KeyCode::Enter));
        assert!(matches!(app.overlay, Some(Overlay::Input(_))));
        app.on_key(key(KeyCode::Esc));
        assert!(app.overlay.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn q_quits_when_nothing_is_running() {
        let (mut app, dir) = scratch();
        app.pending = 0;
        app.on_key(key(KeyCode::Char('q')));
        assert!(app.quit);
        let _ = std::fs::remove_dir_all(dir);
    }
}
