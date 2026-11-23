//! Mega Hack targeting.
//!
//! Geode only runs on 2.2 and up. Below that the tool of the era is Mega Hack, and
//! which release you need is decided entirely by the game build. The table below is
//! the full public Windows history from Absolute's version list, newest first, and it
//! is the only place a release is named - the UI reads it, nothing is hard-coded in
//! the front end.
//!
//! Rules:
//!
//!   - an exact build match returns every release that lists it;
//!   - a build with no row of its own falls back to the newest release that lists an
//!     older build, so nothing ever claims support it does not have;
//!   - anything below 2.2 is a Mega Hack build by definition, and 2.113 and below get
//!     the free v5 family by default - v6 and up are paid.
//!
//! Nothing here downloads Mega Hack. The installers are account-gated (v6 and up) and
//! there is no public direct URL, so the launcher resolves the release and hands the
//! user to Absolute's store, then runs the installer they pick.

use serde::Serialize;

pub const STORE_URL: &str = "https://www.absolllute.com/store/mega-hack";

/// (release, game build, platforms, date, geode, standalone). Newest first.
const RELEASES: &[(&str, &str, &str, &str, bool, bool)] = &[
    ("v9.2.0-alpha.3", "2.2091", "Windows", "2026-10-24", false, true),
    ("v9.2.0-alpha.2", "2.2091", "Windows", "2026-10-23", false, true),
    ("v9.1.3", "2.2081", "Windows • Android", "2026-04-17", true, false),
    ("v9.1.2", "2.2081", "Windows • Android", "2026-04-17", true, false),
    ("v9.1.1", "2.2081", "Windows • Android", "2026-03-11", true, false),
    ("v9.1.0", "2.2081", "Windows • Android", "2026-03-10", true, false),
    ("v9.1.0-beta.8", "2.2081", "Windows", "2026-02-24", true, true),
    ("v9.1.0-beta.7", "2.2081", "Windows", "2026-02-17", true, true),
    ("v9.1.0-beta.6", "2.2081", "Windows", "2026-02-13", false, true),
    ("v9.1.0-beta.5", "2.2081", "Windows", "2026-01-28", false, true),
    ("v9.1.0-beta.4", "2.2081", "Windows", "2026-01-27", false, true),
    ("v9.1.0-beta.3", "2.2081", "Windows", "2026-01-27", false, true),
    ("v9.1.0-beta.2", "2.2081", "Windows", "2026-01-25", false, true),
    ("v9.1.0-beta.1", "2.2081", "Windows", "2026-01-24", false, true),
    ("v9.0.11", "2.2074", "Windows • Android", "2026-01-02", true, false),
    ("v9.0.10", "2.2074", "Windows • Android", "2026-01-02", true, false),
    ("v9.0.9", "2.2074", "Windows • Android", "2025-12-17", true, false),
    ("v9.0.8", "2.2074", "Windows • Android", "2025-12-17", true, false),
    ("v9.0.7", "2.2074", "Windows • Android", "2025-12-09", true, false),
    ("v9.0.6", "2.2074", "Windows • Android", "2025-12-09", true, false),
    ("v9.0.5", "2.2074", "Windows • Android", "2025-12-08", true, false),
    ("v9.0.4", "2.2074", "Windows • Android", "2025-12-08", true, false),
    ("v9.0.3", "2.2074", "Windows • Android", "2025-12-03", true, false),
    ("v9.0.2", "2.2074", "Windows • Android", "2025-12-03", true, false),
    ("v9.0.1", "2.2074", "Windows • Android", "2025-12-01", true, false),
    ("v9.0.0", "2.2074", "Windows • Android", "2025-11-29", true, false),
    ("v8.0.6-alpha.1", "2.2074", "Windows", "2024-11-29", true, true),
    ("v8.0.5-alpha.2", "2.2074", "Windows", "2024-11-13", true, true),
    ("v8.0.5-alpha.1", "2.2074", "Windows", "2024-11-13", false, true),
    ("v8.0.4-alpha.2", "2.2073", "Windows", "2024-11-10", false, true),
    ("v8.0.4-alpha.1", "2.2073", "Windows", "2024-11-09", false, true),
    ("v8.0.3", "2.206", "Windows", "2024-10-22", true, true),
    ("v8.0.2", "2.206", "Windows", "2024-10-09", true, true),
    ("v8.0.1-alpha.10", "2.206", "Windows", "2024-09-30", true, true),
    ("v8.0.1-alpha.9", "2.206", "Windows", "2024-09-18", true, true),
    ("v8.0.1-alpha.8", "2.206", "Windows", "2024-08-15", true, true),
    ("v8.0.1-alpha.7", "2.206", "Windows", "2024-07-20", true, true),
    ("v8.0.1-alpha.6", "2.206", "Windows", "2024-07-11", true, true),
    ("v8.0.1-alpha.5", "2.206", "Windows", "2024-07-10", true, true),
    ("v8.0.1-alpha.4", "2.206", "Windows", "2024-06-23", true, true),
    ("v8.0.1-alpha.3", "2.206", "Windows", "2024-06-23", true, true),
    ("v8.0.1-alpha.2", "2.206", "Windows", "2024-06-13", false, true),
    ("v8.0.1-alpha.1", "2.206", "Windows", "2024-06-11", false, true),
    ("v8.0.0-alpha.13d", "2.204", "Windows", "2024-03-13", true, true),
    ("v8.0.0-alpha.13c", "2.204", "Windows", "2024-03-12", true, true),
    ("v8.0.0-alpha.13b", "2.204", "Windows", "2024-03-11", true, true),
    ("v8.0.0-alpha.12a", "2.204", "Windows", "2024-03-07", true, true),
    ("v8.0.0-alpha.12", "2.204", "Windows", "2024-03-07", true, true),
    ("v8.0.0-alpha.11c", "2.204", "Windows", "2024-02-28", true, true),
    ("v8.0.0-alpha.11a", "2.204", "Windows", "2024-02-18", true, true),
    ("v8.0.0-alpha.11", "2.204", "Windows", "2024-02-17", true, true),
    ("v8.0.0-alpha.10c", "2.204", "Windows", "2024-02-09", true, true),
    ("v8.0.0-alpha.10b", "2.204", "Windows", "2024-02-08", true, true),
    ("v8.0.0-alpha.9b", "2.204", "Windows", "2024-02-06", true, true),
    ("v8.0.0-alpha.9a", "2.204", "Windows", "2024-01-30", true, true),
    ("v8.0.0-alpha.9", "2.204", "Windows", "2024-01-27", true, true),
    ("v8.0.0-alpha.8", "2.204", "Windows", "2024-01-24", true, true),
    ("v8.0.0-alpha.7a", "2.204", "Windows", "2024-01-17", true, true),
    ("v8.0.0-alpha.7", "2.204", "Windows", "2024-01-17", false, true),
    ("v8.0.0-alpha.6e", "2.204", "Windows", "2024-01-12", false, true),
    ("v8.0.0-alpha.6d", "2.203", "Windows", "2024-01-12", false, true),
    ("v8.0.0-alpha.6c", "2.203", "Windows", "2024-01-12", false, true),
    ("v8.0.0-alpha.6b", "2.203", "Windows", "2024-01-12", false, true),
    ("v8.0.0-alpha.6a", "2.202", "Windows", "2024-01-10", false, true),
    ("v8.0.0-alpha.6", "2.202", "Windows", "2024-01-10", false, true),
    ("v8.0.0-alpha.5a", "2.2", "Windows", "2024-01-05", false, true),
    ("v8.0.0-alpha.5", "2.2", "Windows", "2024-01-05", false, true),
    ("v8.0.0-alpha.4a", "2.2", "Windows", "2023-12-29", false, true),
    ("v8.0.0-alpha.4", "2.2", "Windows", "2023-12-28", false, true),
    ("v8.0.0-alpha.3", "2.2", "Windows", "2023-12-24", false, true),
    ("v7.1.1.4", "2.113", "Windows", "2023-11-10", true, false),
    ("v7.1.1.3", "2.113", "Windows", "2023-08-01", true, true),
    ("v7.1.1-GM2", "2.113", "Windows", "2023-06-19", false, true),
    ("v7.1.1-GM1", "2.113", "Windows", "2023-05-20", false, true),
    ("v7.1.0-GM1", "2.113", "Windows", "2022-10-09", false, true),
    ("v7.1.0-beta.7", "2.113", "Windows", "2022-09-27", false, true),
    ("v7.1.0-beta.6", "2.113", "Windows", "2022-09-03", false, true),
    ("v7.1.0-beta.5", "2.113", "Windows", "2022-09-02", false, true),
    ("v7.1.0-beta.4", "2.113", "Windows", "2022-08-17", false, true),
    ("v7.1.0-beta.3", "2.113", "Windows", "2022-08-02", false, true),
    ("v7.1.0-beta.2", "2.113", "Windows", "2022-07-25", false, true),
    ("v7.1.0-beta.1", "2.113", "Windows", "2022-07-20", false, true),
    ("v7.0.0-GM1", "2.113", "Windows", "2021-12-31", false, true),
    ("v7.0.0-beta.5", "2.113", "Windows", "2021-12-16", false, true),
    ("v7.0.0-beta.4", "2.113", "Windows", "2021-11-25", false, true),
    ("v7.0.0-beta.3", "2.113", "Windows", "2021-11-21", false, true),
    ("v7.0.0-beta.2", "2.113", "Windows", "2021-11-21", false, true),
    ("v7.0.0-beta.1", "2.113", "Windows", "2021-11-18", false, true),
    ("v6.3.0-GM1", "2.113", "Windows", "2021-08-09", false, true),
    ("v6.3.0-beta.5", "2.113", "Windows", "2021-07-30", false, true),
    ("v6.3.0-beta.4", "2.113", "Windows", "2021-07-20", false, true),
    ("v6.3.0-beta.3", "2.113", "Windows", "2021-07-11", false, true),
    ("v6.3.0-beta.2", "2.113", "Windows", "2021-07-02", false, true),
    ("v6.3.0-beta.1", "2.113", "Windows", "2021-07-02", false, true),
    ("v6.2.1", "2.113", "Windows", "2021-06-09", false, true),
    ("v6.2.0-GM1", "2.113", "Windows", "2021-03-29", false, true),
    ("v6.2.0-beta.4", "2.113", "Windows", "2021-03-17", false, true),
    ("v6.2.0-beta.3", "2.113", "Windows", "2021-03-07", false, true),
    ("v6.2.0-beta.2", "2.113", "Windows", "2021-03-03", false, true),
    ("v6.2.0-beta.1", "2.113", "Windows", "2021-02-07", false, true),
    ("v6.1.0", "2.113", "Windows", "2020-08-25", false, true),
    ("v6.0.0-2", "2.113", "Windows", "2020-05-14", false, true),
    ("v6.0.0", "2.113", "Windows", "2020-05-14", false, true),
    ("v5.4", "2.113", "Windows", "2019-03-10", false, true),
    ("v5.3", "2.113", "Windows", "2018-06-23", false, true),
    ("v5.2", "2.113", "Windows", "2018-05-28", false, true),
    ("v5.1", "2.113", "Windows", "2018-05-28", false, true),
    ("v5.0", "2.113", "Windows", "2018-05-28", false, true),
    ("v4", "2.102", "Windows", "2017-07-19", false, true),
    ("v3", "2.102", "Windows", "2017-02-14", false, true),
    ("v2", "2.020", "Windows", "2016-11-13", false, true),
    ("v1", "2.020", "Windows", "2016-06-08", false, true),
];

#[derive(Debug, Clone, Serialize)]
pub struct MhRelease {
    /// "v9.1.3"
    pub version: String,
    /// 9
    pub major: u32,
    /// "v9"
    pub major_label: String,
    /// The game build this release targets.
    pub game: String,
    pub platforms: String,
    pub date: String,
    pub geode: bool,
    pub standalone: bool,
    /// v6 and up need a paid Absolute account.
    pub paid: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MegaHackTarget {
    /// Recommended major, "v5" .. "v9".
    pub major: String,
    /// Recommended release, "Mega Hack v9.1.3".
    pub label: String,
    pub paid: bool,
    /// True for builds older than 2.2 - no Geode, Mega Hack only.
    pub below_2_2: bool,
    /// Which game builds the recommendation covers, for display.
    pub covers: String,
    pub store_url: String,
    pub note: String,
    pub recommended: MhRelease,
    /// Every release that lists this build, newest first.
    pub releases: Vec<MhRelease>,
}

/// A Geometry Dash build number, ordered the way the game actually shipped.
///
/// The scheme is `2.<minor digit><sub-build>`, where everything after the minor
/// digit is a *digit string*, not a number: `2.2074` is 2.2 build 074 and it came
/// out before `2.208`, which is 2.2 build 08. Comparing those as integers puts
/// 2.2074 after 2.208 and picks the wrong tool, so the tail is kept as digits and
/// compared lexicographically, shorter first (`2.2` < `2.201`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GdVersion {
    pub minor: u32,
    pub sub: String,
}

/// Geometry Dash version string -> comparable build.
pub fn parse_version(version: &str) -> Option<GdVersion> {
    let trimmed = version.trim();
    let rest = trimmed.strip_prefix("2.").or_else(|| trimmed.strip_prefix("2,"))?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        // "2." alone - treat as 2.0
        return Some(GdVersion {
            minor: 0,
            sub: String::new(),
        });
    }
    let minor = digits.chars().next()?.to_digit(10)?;
    Some(GdVersion {
        minor,
        sub: digits.chars().skip(1).collect(),
    })
}

/// True when the build is older than 2.2 - no Geode, Mega Hack territory.
pub fn below_2_2(version: &str) -> bool {
    match parse_version(version) {
        Some(v) => v.minor < 2,
        None => false,
    }
}

fn major_of(version: &str) -> u32 {
    version
        .trim_start_matches('v')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn to_release((version, game, platforms, date, geode, standalone): &(&str, &str, &str, &str, bool, bool)) -> MhRelease {
    let major = major_of(version);
    MhRelease {
        version: (*version).into(),
        major,
        major_label: format!("v{major}"),
        game: (*game).into(),
        platforms: (*platforms).into(),
        date: (*date).into(),
        geode: *geode,
        standalone: *standalone,
        paid: major >= 6,
    }
}

/// Every release in the table, newest first. Only the tests walk the whole list;
/// the launcher always asks for a specific build.
#[cfg(test)]
pub fn all_releases() -> Vec<MhRelease> {
    RELEASES.iter().map(to_release).collect()
}

fn is_beta(version: &str) -> bool {
    version.contains("alpha") || version.contains("beta")
}

/// The releases the picker offers for a build, newest major first.
///
/// One release per major, and only the newest *stable* one - a release with `alpha`
/// or `beta` in the name is skipped whenever that major has a finished build. The
/// only exception is a major that never shipped stable for the era at all: the 2.2
/// line was alpha-only until v8.0.2, so those builds get the newest alpha of that
/// major rather than an empty panel.
///
/// A release is only offered when the build it targets is at or below the instance,
/// so the picker never points at a release expecting files the instance lacks.
pub fn options_for(version: &str) -> Vec<MhRelease> {
    let Some(key) = parse_version(version) else {
        return Vec::new();
    };

    // 2.1 and below: the v5 -> v7 ladder. 2.2 and up: v8 -> v9.
    let majors: &[u32] = if key.minor >= 2 { &[9, 8] } else { &[7, 6, 5] };
    let needs_standalone = key.minor < 2;
    let last = *majors.last().unwrap_or(&5);

    let mut out: Vec<MhRelease> = Vec::new();
    for &major in majors {
        let all: Vec<MhRelease> = RELEASES
            .iter()
            .filter(|row| major_of(row.0) == major)
            .filter(|row| !needs_standalone || row.5)
            .map(to_release)
            .collect();
        if all.is_empty() {
            continue;
        }

        let mut pool: Vec<MhRelease> = all
            .iter()
            .filter(|r| parse_version(&r.game).map(|g| g <= key).unwrap_or(false))
            .cloned()
            .collect();
        // The oldest major in the ladder is the universal fallback: v5 for 2.1 and
        // below, v8 for the 2.2 line before v9 existed. A build older than every
        // row still needs something to point at.
        if pool.is_empty() && major == last {
            pool = all;
        }
        if pool.is_empty() {
            continue;
        }

        // Table order is newest first, so the first stable row is the newest stable.
        let pick = pool
            .iter()
            .find(|r| !is_beta(&r.version))
            .cloned()
            .unwrap_or_else(|| pool[0].clone());
        out.push(pick);
    }
    out
}

/// The release the launcher starts on: the newest stable major that runs the build.
pub fn recommended(version: &str) -> Option<MhRelease> {
    options_for(version).into_iter().next()
}

/// The major to print next to a build in the version table: "v9", "v8", "v5".
pub fn major_label(version: &str) -> String {
    recommended(version)
        .map(|r| r.major_label)
        .unwrap_or_default()
}

/// Resolve the full Mega Hack picture for a game build.
pub fn target_for(version: &str) -> Option<MegaHackTarget> {
    let releases = options_for(version);
    let recommended = releases.first()?.clone();
    let below = below_2_2(version);

    let note = if recommended.paid {
        "Paid. Sign in at Absolute's store, then run the installer it gives you.".to_string()
    } else if below {
        "Free release. Standalone, no Geode, no account needed.".to_string()
    } else {
        "Free release. Pick the standalone or Geode build from Absolute's store.".to_string()
    };

    Some(MegaHackTarget {
        major: recommended.major_label.clone(),
        label: format!("Mega Hack {}", recommended.version),
        paid: recommended.paid,
        below_2_2: below,
        covers: recommended.game.clone(),
        store_url: STORE_URL.into(),
        note,
        recommended,
        releases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering_matches_history() {
        // The tail after the minor digit is a digit string, not a number: 2.2074 is
        // 2.2 build 074 and shipped before 2.208 (build 08).
        let v = |s: &str| parse_version(s).unwrap();
        assert_eq!(v("2.113").minor, 1);
        assert_eq!(v("2.113").sub, "13");
        assert_eq!(v("2.2074").sub, "074");
        assert_eq!(v("2.208").sub, "08");
        assert!(v("2.113") < v("2.2"));
        assert!(v("2.2") < v("2.201"));
        assert!(v("2.201") < v("2.207"));
        assert!(v("2.207") < v("2.2074"));
        assert!(v("2.2074") < v("2.208"));
        assert!(v("2.208") < v("2.2081"));
        assert!(v("2.2081") < v("2.209"));
        assert!(v("2.2081") < v("2.2091"));
        assert!(v("2.11") < v("2.113"));
        assert!(v("2.103") < v("2.11"));
        assert!(v("2.01") < v("2.012"));
        assert!(v("2.012") < v("2.02"));
    }

    #[test]
    fn every_bundled_build_resolves_to_a_release() {
        for build in crate::manifests::BUNDLED {
            let target = target_for(build.version)
                .unwrap_or_else(|| panic!("{} has no Mega Hack target", build.version));
            assert!(
                !target.releases.is_empty(),
                "{} resolved to no releases",
                build.version
            );
            // One option per major, no duplicates, newest major first.
            let majors: Vec<u32> = target.releases.iter().map(|r| r.major).collect();
            let mut sorted = majors.clone();
            sorted.sort_unstable();
            sorted.reverse();
            assert_eq!(majors, sorted, "{} options out of order", build.version);
            sorted.dedup();
            assert_eq!(sorted.len(), majors.len(), "{} repeats a major", build.version);

            // Nothing older than 2.2 may point at a Geode-only release.
            if target.below_2_2 {
                for release in &target.releases {
                    assert!(
                        release.standalone,
                        "{} offers {} which has no standalone build",
                        build.version, release.version
                    );
                }
            }
        }
    }

    #[test]
    fn stable_only_per_major() {
        // No alpha or beta in any option, unless that major never shipped stable for
        // the era - the 2.2 line was alpha-only until v8.0.2.
        for build in ["2.113", "2.112", "2.0", "2.206", "2.2074", "2.2081"] {
            for release in options_for(build) {
                assert!(
                    !release.version.contains("alpha") && !release.version.contains("beta"),
                    "{build} offered {}",
                    release.version
                );
            }
        }
        // v9 is stable-only: v9.1.3, never a v9.1.0-beta.
        assert_eq!(options_for("2.2081")[0].version, "v9.1.3");
        // v8 has stable rows, so 2.2074 gets v8.0.3, not v8.0.6-alpha.1.
        assert_eq!(options_for("2.2074")[1].version, "v8.0.3");
    }

    #[test]
    fn the_2_1_ladder_is_v7_v6_v5() {
        let opts = options_for("2.113");
        let versions: Vec<&str> = opts.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["v7.1.1.3", "v6.3.0-GM1", "v5.4"]);
        // v7.1.1.4 is Geode-only, and there is no Geode below 2.2, so it is not offered.
        assert!(!versions.contains(&"v7.1.1.4"));
        // Newest first, and the newest is the default.
        assert_eq!(recommended("2.113").unwrap().version, "v7.1.1.3");
        // Older builds fall back to the free v5 alone.
        assert_eq!(
            options_for("2.112").iter().map(|r| r.version.as_str()).collect::<Vec<_>>(),
            ["v5.4"]
        );
        assert_eq!(
            options_for("2.0").iter().map(|r| r.version.as_str()).collect::<Vec<_>>(),
            ["v5.4"]
        );
    }

    #[test]
    fn modern_builds_follow_the_table() {
        assert_eq!(target_for("2.2081").unwrap().recommended.version, "v9.1.3");
        assert_eq!(target_for("2.2074").unwrap().recommended.version, "v9.0.11");
        assert_eq!(target_for("2.206").unwrap().recommended.version, "v8.0.3");
        assert_eq!(target_for("2.204").unwrap().recommended.version, "v8.0.0-alpha.13d");
        assert_eq!(target_for("2.2").unwrap().recommended.version, "v8.0.0-alpha.5a");
        assert!(target_for("2.2081").unwrap().paid);
        assert!(target_for("2.113").unwrap().paid); // v7.1.1.3 is paid
    }

    #[test]
    fn gaps_fall_back_to_an_older_target() {
        // 2.208 has no row; 2.2074 does, and it is older.
        assert_eq!(target_for("2.208").unwrap().recommended.game, "2.2074");
        // 2.201 sits between 2.2 and 2.202.
        assert_eq!(target_for("2.201").unwrap().recommended.game, "2.2");
        // 2.2071 predates 2.2073, so it cannot borrow that release.
        assert_eq!(target_for("2.2071").unwrap().recommended.game, "2.206");
        assert!(target_for("build 26002420").is_none());
    }

    #[test]
    fn geode_gate() {
        assert!(below_2_2("2.113"));
        assert!(below_2_2("2.0"));
        assert!(!below_2_2("2.2"));
        assert!(!below_2_2("2.2081"));
        assert!(!below_2_2(""));
    }

    #[test]
    fn table_is_newest_first_and_labelled() {
        let all = all_releases();
        assert!(all.len() > 100);
        for pair in all.windows(2) {
            assert!(
                pair[0].date >= pair[1].date,
                "{} before {}",
                pair[0].version,
                pair[1].version
            );
        }
        for release in &all {
            assert_eq!(release.major_label, format!("v{}", release.major));
            assert_eq!(release.paid, release.major >= 6);
        }
    }
}
