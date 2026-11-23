//! The settings screen as data: one row per knob, with the same value, hint and
//! step logic the view and the key handler both use.

use crate::config::{DepotBackend, Settings};
use crate::tui::theme;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Toggle,
    /// A fixed list, stepped with left and right.
    Choice,
    /// A number stepped with left and right.
    Number,
    /// Free text, edited in a prompt.
    Text,
    /// Shown but not editable here.
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Theme,
    Tui,
    ConfirmRemove,
    Backend,
    MaxDownloads,
    PurgeCache,
    RememberUsername,
    InstancesDir,
    GameArgs,
    DataDir,
}

impl Setting {
    pub const ALL: [Setting; 10] = [
        Setting::Theme,
        Setting::Tui,
        Setting::ConfirmRemove,
        Setting::Backend,
        Setting::MaxDownloads,
        Setting::PurgeCache,
        Setting::RememberUsername,
        Setting::InstancesDir,
        Setting::GameArgs,
        Setting::DataDir,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Theme => "theme",
            Setting::Tui => "tui by default",
            Setting::ConfirmRemove => "confirm remove",
            Setting::Backend => "depot backend",
            Setting::MaxDownloads => "max downloads",
            Setting::PurgeCache => "purge depot cache",
            Setting::RememberUsername => "remember username",
            Setting::InstancesDir => "instances folder",
            Setting::GameArgs => "game arguments",
            Setting::DataDir => "data folder",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Setting::Theme | Setting::Backend => Kind::Choice,
            Setting::MaxDownloads => Kind::Number,
            Setting::Tui
            | Setting::ConfirmRemove
            | Setting::PurgeCache
            | Setting::RememberUsername => Kind::Toggle,
            Setting::InstancesDir | Setting::GameArgs => Kind::Text,
            Setting::DataDir => Kind::ReadOnly,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Setting::Theme => "palette for this screen, applies as you step",
            Setting::Tui => "bare `gd` opens this screen instead of help",
            Setting::ConfirmRemove => "ask before a remove that deletes files",
            Setting::Backend => "which tool pulls the depots",
            Setting::MaxDownloads => "parallel chunk connections, 1 to 32",
            Setting::PurgeCache => "delete .DepotDownloader after a good pull",
            Setting::RememberUsername => "prefill the cached Steam account name",
            Setting::InstancesDir => "where instance folders go, empty for the default",
            Setting::GameArgs => "extra flags appended to GeometryDash.exe",
            Setting::DataDir => "read only, set it with --data-dir",
        }
    }

    pub fn value(self, settings: &Settings, data_dir: &Path) -> String {
        match self {
            Setting::Theme => settings.tui_theme.clone(),
            Setting::Tui => on_off(settings.tui),
            Setting::ConfirmRemove => on_off(settings.tui_confirm_remove),
            Setting::Backend => match settings.depot_backend {
                DepotBackend::DepotDownloader => "depotdownloader".into(),
                DepotBackend::SteamCmd => "steamcmd".into(),
            },
            Setting::MaxDownloads => settings.max_downloads.to_string(),
            Setting::PurgeCache => on_off(settings.purge_depot_cache),
            Setting::RememberUsername => on_off(settings.remember_username),
            Setting::InstancesDir => {
                if settings.instances_dir.trim().is_empty() {
                    "(default)".into()
                } else {
                    settings.instances_dir.clone()
                }
            }
            Setting::GameArgs => {
                if settings.game_args.trim().is_empty() {
                    "(none)".into()
                } else {
                    settings.game_args.clone()
                }
            }
            Setting::DataDir => data_dir.display().to_string(),
        }
    }

    /// Step a choice or a number. Returns true when the value changed and the
    /// caller should save.
    pub fn step(self, settings: &mut Settings, delta: i32) -> bool {
        match self {
            Setting::Theme => {
                let name = if delta < 0 {
                    theme::prev(&settings.tui_theme)
                } else {
                    theme::next(&settings.tui_theme)
                };
                settings.tui_theme = name.to_string();
                true
            }
            Setting::Backend => {
                settings.depot_backend = match settings.depot_backend {
                    DepotBackend::DepotDownloader => DepotBackend::SteamCmd,
                    DepotBackend::SteamCmd => DepotBackend::DepotDownloader,
                };
                true
            }
            Setting::MaxDownloads => {
                let next = (settings.max_downloads as i32 + delta).clamp(1, 32) as u32;
                if next == settings.max_downloads {
                    return false;
                }
                settings.max_downloads = next;
                true
            }
            Setting::Tui
            | Setting::ConfirmRemove
            | Setting::PurgeCache
            | Setting::RememberUsername => {
                let slot = match self {
                    Setting::Tui => &mut settings.tui,
                    Setting::ConfirmRemove => &mut settings.tui_confirm_remove,
                    Setting::PurgeCache => &mut settings.purge_depot_cache,
                    _ => &mut settings.remember_username,
                };
                *slot = !*slot;
                true
            }
            Setting::InstancesDir | Setting::GameArgs | Setting::DataDir => false,
        }
    }

    /// Starting text for a prompt, or None when the row is not a text row.
    pub fn edit_text(self, settings: &Settings) -> Option<String> {
        match self {
            Setting::InstancesDir => Some(settings.instances_dir.clone()),
            Setting::GameArgs => Some(settings.game_args.clone()),
            _ => None,
        }
    }

    /// Write text back into the settings. Returns true when something changed.
    pub fn set_text(self, settings: &mut Settings, text: &str) -> bool {
        let next = text.trim().to_string();
        match self {
            Setting::InstancesDir => {
                if settings.instances_dir == next {
                    return false;
                }
                settings.instances_dir = next;
                true
            }
            Setting::GameArgs => {
                if settings.game_args == next {
                    return false;
                }
                settings.game_args = next;
                true
            }
            _ => false,
        }
    }
}

fn on_off(value: bool) -> String {
    if value {
        "on".into()
    } else {
        "off".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_steps_through_every_palette() {
        let mut settings = Settings::default();
        let start = settings.tui_theme.clone();
        for _ in 0..theme::THEMES.len() {
            assert!(Setting::Theme.step(&mut settings, 1));
        }
        assert_eq!(settings.tui_theme, start);
    }

    #[test]
    fn downloads_stay_inside_the_range() {
        let mut settings = Settings::default();
        settings.max_downloads = 1;
        assert!(!Setting::MaxDownloads.step(&mut settings, -1));
        assert_eq!(settings.max_downloads, 1);
        settings.max_downloads = 32;
        assert!(!Setting::MaxDownloads.step(&mut settings, 1));
        assert_eq!(settings.max_downloads, 32);
    }

    #[test]
    fn text_rows_round_trip() {
        let mut settings = Settings::default();
        assert!(Setting::InstancesDir.set_text(&mut settings, "  D:\\gd  "));
        assert_eq!(settings.instances_dir, "D:\\gd");
        assert!(!Setting::InstancesDir.set_text(&mut settings, "D:\\gd"));
        assert_eq!(
            Setting::InstancesDir.value(&settings, Path::new("C:\\data")),
            "D:\\gd"
        );
    }

    #[test]
    fn toggles_flip() {
        let mut settings = Settings::default();
        let before = settings.tui;
        assert!(Setting::Tui.step(&mut settings, 1));
        assert_eq!(settings.tui, !before);
    }
}
