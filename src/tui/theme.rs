//! Palettes for the full screen interface.
//!
//! Six of them, all dark-background. `paper` is the near-white one for people who
//! want less colour. A name that is not in the table falls back to the first, so
//! a hand-edited config cannot leave the screen invisible.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub accent: Color,
    pub accent_dim: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub sel_fg: Color,
    pub sel_bg: Color,
    pub ok: Color,
    pub warn: Color,
    pub error: Color,
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "amber",
        accent: Color::Rgb(255, 176, 59),
        accent_dim: Color::Rgb(120, 80, 25),
        text: Color::Rgb(222, 216, 205),
        muted: Color::Rgb(122, 116, 105),
        border: Color::Rgb(72, 62, 46),
        sel_fg: Color::Rgb(22, 16, 6),
        sel_bg: Color::Rgb(255, 176, 59),
        ok: Color::Rgb(126, 200, 126),
        warn: Color::Rgb(240, 200, 90),
        error: Color::Rgb(240, 110, 100),
    },
    Theme {
        name: "phosphor",
        accent: Color::Rgb(122, 255, 142),
        accent_dim: Color::Rgb(38, 92, 48),
        text: Color::Rgb(198, 240, 204),
        muted: Color::Rgb(96, 132, 102),
        border: Color::Rgb(42, 74, 48),
        sel_fg: Color::Rgb(4, 22, 8),
        sel_bg: Color::Rgb(122, 255, 142),
        ok: Color::Rgb(122, 255, 142),
        warn: Color::Rgb(220, 230, 120),
        error: Color::Rgb(255, 120, 110),
    },
    Theme {
        name: "ice",
        accent: Color::Rgb(110, 200, 255),
        accent_dim: Color::Rgb(32, 74, 102),
        text: Color::Rgb(212, 228, 240),
        muted: Color::Rgb(104, 124, 140),
        border: Color::Rgb(44, 66, 84),
        sel_fg: Color::Rgb(4, 16, 26),
        sel_bg: Color::Rgb(110, 200, 255),
        ok: Color::Rgb(120, 220, 190),
        warn: Color::Rgb(235, 210, 130),
        error: Color::Rgb(255, 130, 140),
    },
    Theme {
        name: "violet",
        accent: Color::Rgb(190, 140, 255),
        accent_dim: Color::Rgb(74, 52, 104),
        text: Color::Rgb(226, 218, 240),
        muted: Color::Rgb(124, 112, 140),
        border: Color::Rgb(62, 50, 82),
        sel_fg: Color::Rgb(14, 6, 26),
        sel_bg: Color::Rgb(190, 140, 255),
        ok: Color::Rgb(140, 220, 160),
        warn: Color::Rgb(235, 200, 130),
        error: Color::Rgb(255, 120, 150),
    },
    Theme {
        name: "ember",
        accent: Color::Rgb(255, 110, 70),
        accent_dim: Color::Rgb(104, 44, 28),
        text: Color::Rgb(236, 216, 206),
        muted: Color::Rgb(134, 110, 100),
        border: Color::Rgb(84, 48, 38),
        sel_fg: Color::Rgb(26, 8, 4),
        sel_bg: Color::Rgb(255, 110, 70),
        ok: Color::Rgb(150, 210, 130),
        warn: Color::Rgb(245, 200, 110),
        error: Color::Rgb(255, 90, 80),
    },
    Theme {
        name: "paper",
        accent: Color::Rgb(236, 236, 226),
        accent_dim: Color::Rgb(96, 96, 90),
        text: Color::Rgb(222, 222, 214),
        muted: Color::Rgb(142, 142, 134),
        border: Color::Rgb(92, 92, 86),
        sel_fg: Color::Rgb(14, 14, 12),
        sel_bg: Color::Rgb(236, 236, 226),
        ok: Color::Rgb(180, 210, 170),
        warn: Color::Rgb(220, 210, 160),
        error: Color::Rgb(230, 140, 130),
    },
];

pub fn by_name(name: &str) -> &'static Theme {
    let wanted = name.trim();
    THEMES
        .iter()
        .find(|theme| theme.name.eq_ignore_ascii_case(wanted))
        .unwrap_or(&THEMES[0])
}

/// Next palette in the table, wrapping.
pub fn next(name: &str) -> &'static str {
    let index = THEMES
        .iter()
        .position(|theme| theme.name.eq_ignore_ascii_case(name.trim()))
        .unwrap_or(0);
    THEMES[(index + 1) % THEMES.len()].name
}

/// Previous palette, wrapping.
pub fn prev(name: &str) -> &'static str {
    let index = THEMES
        .iter()
        .position(|theme| theme.name.eq_ignore_ascii_case(name.trim()))
        .unwrap_or(0);
    THEMES[(index + THEMES.len() - 1) % THEMES.len()].name
}

pub fn names() -> Vec<&'static str> {
    THEMES.iter().map(|theme| theme.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_fall_back_to_the_first_palette() {
        assert_eq!(by_name("nope").name, THEMES[0].name);
        assert_eq!(by_name("").name, THEMES[0].name);
    }

    #[test]
    fn cycling_wraps_both_ways() {
        let first = THEMES[0].name;
        let last = THEMES[THEMES.len() - 1].name;
        assert_eq!(next(last), first);
        assert_eq!(prev(first), last);
        assert_eq!(next(first), THEMES[1].name);
    }
}
