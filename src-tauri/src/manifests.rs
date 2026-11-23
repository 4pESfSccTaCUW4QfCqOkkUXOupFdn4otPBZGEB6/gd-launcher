//! Bundled Geometry Dash depot manifest catalogue.
//!
//! Version -> Windows depot manifest gid for app 322170 / depot 322171. Dates are
//! the Steam publish dates. The list is deliberately static: Valve does not serve
//! depot history, so this is the offline copy the picker reads from. The live
//! public manifest is fetched separately and shown above these rows.

pub const GD_DEPOT: &str = "322171";

pub struct Bundled {
    pub version: &'static str,
    pub date: &'static str,
    pub manifest: &'static str,
}

/// Newest first.
pub const BUNDLED: &[Bundled] = &[
    Bundled { version: "2.2081", date: "2026-01-21", manifest: "3816559102876907245" },
    Bundled { version: "2.208",  date: "2026-01-19", manifest: "4708369212204820145" },
    Bundled { version: "2.2074", date: "2024-11-12", manifest: "7678373534998244044" },
    Bundled { version: "2.2073", date: "2024-11-09", manifest: "7061773846290027563" },
    Bundled { version: "2.2072", date: "2024-11-09", manifest: "6397429455085000806" },
    Bundled { version: "2.2071", date: "2024-11-09", manifest: "1924617755796511196" },
    Bundled { version: "2.207",  date: "2024-11-08", manifest: "7323959135167311541" },
    Bundled { version: "2.206",  date: "2024-06-01", manifest: "7903404280228366327" },
    Bundled { version: "2.204",  date: "2024-01-12", manifest: "2530486154587189554" },
    Bundled { version: "2.203",  date: "2024-01-11", manifest: "8259882692733807559" },
    Bundled { version: "2.202",  date: "2024-01-07", manifest: "2018952014081804638" },
    Bundled { version: "2.201",  date: "2024-01-07", manifest: "6284244757349794393" },
    Bundled { version: "2.2",    date: "2023-12-19", manifest: "3500860979376500740" },
    Bundled { version: "2.113",  date: "2017-11-20", manifest: "641118531549408775" },
    Bundled { version: "2.112",  date: "2017-11-14", manifest: "220851016460276263" },
    Bundled { version: "2.111",  date: "2017-11-13", manifest: "3295169357678211274" },
    Bundled { version: "2.11",   date: "2017-11-12", manifest: "4631472650007457724" },
    Bundled { version: "2.103",  date: "2017-01-18", manifest: "4058039153940377450" },
    Bundled { version: "2.102",  date: "2017-01-17", manifest: "3342223721911279245" },
    Bundled { version: "2.101",  date: "2017-01-17", manifest: "2062255848267380058" },
    Bundled { version: "2.012",  date: "2015-10-15", manifest: "631879647548207429" },
    Bundled { version: "2.011",  date: "2015-09-24", manifest: "542142667514100820" },
    Bundled { version: "2.01",   date: "2015-09-24", manifest: "1327679890501772194" },
    Bundled { version: "2.001",  date: "2015-08-26", manifest: "2135166447199899913" },
    Bundled { version: "2.0",    date: "2015-08-26", manifest: "1050808584461628025" },
];

/// Unix seconds -> `YYYY-MM-DD`, so user-added rows slot into the same ordering.
pub fn civil_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
