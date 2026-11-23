//! Drawing for the full screen interface.
//!
//! Everything here reads `App` and paints. No key handling, no disk, no network.

use super::{App, Confirm, Input, Overlay, Tab};
use crate::config::DepotBackend;
use crate::{manifests, mh, paths, util};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    let mut constraints = vec![
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(6),
    ];
    if app.show_log {
        constraints.push(Constraint::Length(8));
    }
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));

    let chunks = Layout::vertical(constraints).split(frame.area());

    header(frame, app, chunks[0]);
    tabs(frame, app, chunks[1]);

    let body = chunks[3];
    let title = match app.tab {
        Tab::Instances => format!("instances · {}", app.instances.len()),
        Tab::Versions => format!("versions · {} builds", manifests::BUNDLED.len()),
        Tab::Mods => match app.selected_instance() {
            Some(view) => format!("mods · {} {}", view.base.name, view.base.version),
            None => "mods".into(),
        },
        Tab::MegaHack => match app.selected_instance() {
            Some(view) => format!("mega hack · {}", view.base.version),
            None => "mega hack".into(),
        },
        Tab::Steam => "steam".into(),
        Tab::Settings => "settings".into(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(&block, body);
    let inner = block.inner(body);

    match app.tab {
        Tab::Instances => draw_instances(frame, app, inner),
        Tab::Versions => draw_versions(frame, app, inner),
        Tab::Mods => draw_mods(frame, app, inner),
        Tab::MegaHack => draw_mega_hack(frame, app, inner),
        Tab::Steam => draw_steam(frame, app, inner),
        Tab::Settings => draw_settings(frame, app, inner),
    }

    if app.show_log {
        draw_log(frame, app, chunks[4]);
    }
    status_line(frame, app, chunks[chunks.len() - 2]);
    keys_line(frame, app, chunks[chunks.len() - 1]);

    if let Some(qr) = &app.qr {
        draw_qr(frame, app, qr);
    }
    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => draw_help(frame, app),
            Overlay::Confirm(confirm) => draw_confirm(frame, app, confirm),
            Overlay::Input(input) => draw_input(frame, app, input),
        }
    }
}

fn header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let columns = Layout::horizontal([Constraint::Min(30), Constraint::Length(52)]).split(area);

    let backend = match app.cfg.settings.depot_backend {
        DepotBackend::DepotDownloader => "depotdownloader",
        DepotBackend::SteamCmd => "steamcmd",
    };
    let left = Line::from(vec![
        Span::styled(
            " gd ",
            Style::new()
                .fg(theme.sel_fg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", env!("CARGO_PKG_VERSION")),
            Style::new().fg(theme.muted),
        ),
        Span::styled(app.theme.name, Style::new().fg(theme.accent)),
        Span::styled("  ·  ", Style::new().fg(theme.border)),
        Span::styled(&app.session, Style::new().fg(theme.text)),
        Span::styled("  ·  ", Style::new().fg(theme.border)),
        Span::styled(backend, Style::new().fg(theme.muted)),
    ]);
    frame.render_widget(Paragraph::new(left), columns[0]);

    if let Some(job) = &app.job {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            [(app.frames / 2) as usize % 10];
        let percent = job
            .percent
            .map(|percent| format!(" {percent:>5.1}%"))
            .unwrap_or_default();
        let queued = if app.pending > 1 {
            format!(" +{}", app.pending - 1)
        } else {
            String::new()
        };
        let line = format!("{spinner} {}{percent}{queued} ", job.label);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(line, Style::new().fg(theme.accent))))
                .alignment(Alignment::Right),
            columns[1],
        );
    }
}

fn tabs(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let mut spans = vec![Span::raw(" ")];
    for (index, tab) in Tab::ALL.iter().enumerate() {
        let active = *tab == app.tab;
        let label = format!(" {} {} ", index + 1, tab.title());
        spans.push(Span::styled(
            label,
            if active {
                Style::new()
                    .fg(theme.sel_fg)
                    .bg(theme.sel_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.muted)
            },
        ));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_instances(frame: &mut Frame, app: &App, area: Rect) {
    if app.instances.is_empty() {
        empty(
            frame,
            app,
            area,
            "no instances yet\n\npress 2 to pick a build, or i from here",
        );
        return;
    }

    let theme = app.theme;
    let header = Row::new(["ID", "NAME", "VERSION", "STATE", "SIZE", "MODS"])
        .style(Style::new().fg(theme.muted));

    let rows = app.instances.iter().map(|view| {
        let (state, colour) = if !view.exists {
            ("missing", theme.error)
        } else if !view.has_exe {
            ("no exe", theme.error)
        } else if view.geode {
            ("geode", theme.ok)
        } else {
            ("ready", theme.text)
        };
        Row::new(vec![
            Cell::from(short(&view.base.id)),
            Cell::from(truncate(&view.base.name, 40)),
            Cell::from(view.base.version.clone()),
            Cell::from(state).style(Style::new().fg(colour)),
            right(view.size_human.clone()),
            right(view.mod_count.to_string()),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(9),
            Constraint::Min(18),
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    state.select(Some(app.instance_sel));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_versions(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let header = Row::new(["VERSION", "RELEASED", "MANIFEST"])
        .style(Style::new().fg(theme.muted));
    let rows = manifests::BUNDLED.iter().map(|entry| {
        Row::new(vec![
            Cell::from(entry.version),
            Cell::from(entry.date),
            Cell::from(entry.manifest),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    state.select(Some(app.version_sel));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_mods(frame: &mut Frame, app: &App, area: Rect) {
    let Some(view) = app.selected_instance() else {
        empty(frame, app, area, "pick an instance first\n\npress 1");
        return;
    };

    let theme = app.theme;
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let mode = if app.mods.searching {
        format!(" · {} search results", app.mods.total)
    } else {
        format!(" · {} installed", app.mods.installed.len())
    };
    let head = Line::from(vec![
        Span::styled(
            format!(" {} {}", view.base.name, view.base.version),
            Style::new().fg(theme.text),
        ),
        Span::styled(mode, Style::new().fg(theme.muted)),
        Span::styled(
            if view.geode {
                "  ·  loader present"
            } else {
                "  ·  no loader"
            },
            Style::new().fg(if view.geode { theme.ok } else { theme.warn }),
        ),
    ]);
    frame.render_widget(Paragraph::new(head), chunks[0]);

    if app.mods.searching {
        if app.mods.results.is_empty() {
            empty(frame, app, chunks[1], "no results\n\npress s to search again");
            return;
        }
        let header =
            Row::new(["ID", "NAME", "VERSION", "DOWNLOADS"]).style(Style::new().fg(theme.muted));
        let rows = app.mods.results.iter().map(|card| {
            Row::new(vec![
                Cell::from(truncate(&card.id, 34)),
                Cell::from(truncate(&card.name, 32)),
                Cell::from(card.version.clone()),
                right(card.downloads.to_string()),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(36),
                Constraint::Min(20),
                Constraint::Length(12),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
        .highlight_symbol("› ");
        let mut state = TableState::default();
        state.select(Some(app.mods.sel));
        frame.render_stateful_widget(table, chunks[1], &mut state);
        return;
    }

    if app.mods.installed.is_empty() {
        empty(
            frame,
            app,
            chunks[1],
            "no mods installed\n\npress s to search the Geode index",
        );
        return;
    }

    let header = Row::new(["ID", "VERSION", "NAME", "SIZE"]).style(Style::new().fg(theme.muted));
    let rows = app.mods.installed.iter().map(|module| {
        Row::new(vec![
            Cell::from(truncate(&module.id, 36)),
            Cell::from(module.version.clone()),
            Cell::from(truncate(&module.name, 34)),
            right(util::human_bytes(module.size)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(38),
            Constraint::Length(14),
            Constraint::Min(20),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
    .highlight_symbol("› ");
    let mut state = TableState::default();
    state.select(Some(app.mods.sel));
    frame.render_stateful_widget(table, chunks[1], &mut state);
}

fn draw_mega_hack(frame: &mut Frame, app: &App, area: Rect) {
    let Some(view) = app.selected_instance() else {
        empty(frame, app, area, "pick an instance first\n\npress 1");
        return;
    };

    let theme = app.theme;
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let licence = if app.licence.signed_in {
        format!("licence {} on disk", app.licence.masked)
    } else {
        "no Absolute account, paid releases will not install".to_string()
    };
    let installed = match &app.mh_installed {
        Some((release, files)) => format!("{release}  {files} files"),
        None => "none".into(),
    };
    let head = vec![
        Line::from(vec![
            Span::styled("instance  ", Style::new().fg(theme.muted)),
            Span::styled(
                format!("{}  {}", short(&view.base.id), view.base.version),
                Style::new().fg(theme.text),
            ),
            Span::styled("   installed  ", Style::new().fg(theme.muted)),
            Span::styled(installed, Style::new().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("licence   ", Style::new().fg(theme.muted)),
            Span::styled(
                licence,
                Style::new().fg(if app.licence.signed_in {
                    theme.ok
                } else {
                    theme.warn
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("catalogue ", Style::new().fg(theme.muted)),
            Span::styled(
                if app.mh_loading {
                    "loading".to_string()
                } else {
                    format!("{} windows bundles cached", app.mh.bundles.len())
                },
                Style::new().fg(theme.muted),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(head), chunks[0]);

    let options = mh::options_for(&view.base.version);
    if options.is_empty() {
        empty(
            frame,
            app,
            chunks[1],
            "no Mega Hack release targets this build\n\nthe store link is in the README",
        );
        return;
    }

    let recommended = mh::recommended(&view.base.version).map(|release| release.version);
    let header = Row::new(["RELEASE", "TARGET", "CHANNEL", "COST", "PACKAGE"])
        .style(Style::new().fg(theme.muted));

    let rows = options.iter().map(|release| {
        let channel = match (release.geode, release.standalone) {
            (true, true) => "geode+both",
            (true, false) => "geode",
            (false, true) => "standalone",
            (false, false) => "?",
        };
        let package = app
            .mh
            .bundles
            .iter()
            .find(|bundle| {
                bundle.gjver == view.base.version && bundle.version == release.version
            })
            .map(|bundle| if bundle.geode { "geode" } else { "yes" })
            .unwrap_or("no");
        let name = if Some(&release.version) == recommended.as_ref() {
            format!("{} *", release.version)
        } else {
            release.version.clone()
        };
        Row::new(vec![
            Cell::from(name),
            Cell::from(release.game.clone()),
            Cell::from(channel),
            Cell::from(if release.paid { "paid" } else { "free" }),
            Cell::from(package).style(Style::new().fg(if package == "no" {
                theme.error
            } else {
                theme.ok
            })),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    state.select(Some(app.mh.sel));
    frame.render_stateful_widget(table, chunks[1], &mut state);
}

fn draw_steam(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let chunks = Layout::vertical([Constraint::Min(4), Constraint::Length(9)]).split(area);

    if app.steam.installs.is_empty() {
        empty(frame, app, chunks[0], "Steam has no Geometry Dash library entry");
    } else {
        let header =
            Row::new(["#", "BUILD", "SIZE", "PATH"]).style(Style::new().fg(theme.muted));
        let rows = app.steam.installs.iter().enumerate().map(|(index, install)| {
            Row::new(vec![
                Cell::from(index.to_string()),
                Cell::from(if install.build_id.is_empty() {
                    "?".to_string()
                } else {
                    install.build_id.clone()
                }),
                Cell::from(util::human_bytes(install.size_on_disk)),
                Cell::from(truncate(&install.path, 70)),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Length(14),
                Constraint::Length(9),
                Constraint::Min(30),
            ],
        )
        .header(header)
        .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
        .highlight_symbol("› ");
        let mut state = TableState::default();
        state.select(Some(app.steam.sel));
        frame.render_stateful_widget(table, chunks[0], &mut state);
    }

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        " accounts",
        Style::new().fg(theme.accent),
    )));
    if app.steam.accounts.is_empty() {
        lines.push(Line::from(Span::styled(
            "   none in loginusers.vdf",
            Style::new().fg(theme.muted),
        )));
    } else {
        for account in &app.steam.accounts {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {:<24}", truncate(&account.account_name, 24)),
                    Style::new().fg(theme.text),
                ),
                Span::styled(account.persona.clone(), Style::new().fg(theme.muted)),
                Span::styled(
                    if account.active { "  (active)" } else { "" },
                    Style::new().fg(theme.ok),
                ),
            ]));
        }
    }

    let session = paths::session(&app.dir);
    lines.push(Line::from(vec![
        Span::styled(" session   ", Style::new().fg(theme.muted)),
        Span::styled(
            if session.signed_in {
                format!("{}  {}", session.username, session.path)
            } else {
                "no cached login, press s to scan a QR".into()
            },
            Style::new().fg(if session.signed_in {
                theme.text
            } else {
                theme.warn
            }),
        ),
    ]));

    let toolchain = match paths::resolve_backend(&app.dir, &app.cfg.settings) {
        Ok((_, exe)) => exe.display().to_string(),
        Err(err) => format!("{err} - fetched on first install"),
    };
    lines.push(Line::from(vec![
        Span::styled(" toolchain ", Style::new().fg(theme.muted)),
        Span::styled(toolchain, Style::new().fg(theme.muted)),
    ]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::TOP).border_style(Style::new().fg(theme.border)))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let chunks = Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).split(area);

    let header = Row::new(["SETTING", "VALUE", "KIND"]).style(Style::new().fg(theme.muted));
    let rows = super::settings::Setting::ALL.iter().map(|setting| {
        let kind = match setting.kind() {
            super::settings::Kind::Toggle => "toggle",
            super::settings::Kind::Choice => "choice",
            super::settings::Kind::Number => "number",
            super::settings::Kind::Text => "text",
            super::settings::Kind::ReadOnly => "read only",
        };
        Row::new(vec![
            Cell::from(setting.label()),
            Cell::from(truncate(&setting.value(&app.cfg.settings, &app.dir), 60)),
            Cell::from(kind).style(Style::new().fg(theme.muted)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Min(24),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(theme.sel_bg).fg(theme.sel_fg))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    state.select(Some(app.settings_sel));
    frame.render_stateful_widget(table, chunks[0], &mut state);

    let setting = super::settings::Setting::ALL[app.settings_sel];
    let hint = Line::from(vec![
        Span::styled(
            format!(" {}  ", setting.label()),
            Style::new().fg(theme.accent),
        ),
        Span::styled(setting.hint(), Style::new().fg(theme.muted)),
    ]);
    frame.render_widget(
        Paragraph::new(hint)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(theme.border)),
            )
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .title(Span::styled(
            " activity ",
            Style::new().fg(theme.accent_dim),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(height)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| {
            Line::from(Span::styled(
                format!(" {}", truncate(line, inner.width as usize)),
                Style::new().fg(if line.starts_with("error:") {
                    theme.error
                } else {
                    theme.muted
                }),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn status_line(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let colour = if app.status.starts_with("error") {
        theme.error
    } else {
        theme.text
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}", truncate(&app.status, area.width.saturating_sub(1) as usize)),
            Style::new().fg(colour),
        ))),
        area,
    );
}

fn keys_line(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let keys = match app.tab {
        Tab::Instances => {
            "enter launch  m mega hack  l loader  d remove  o folder  i install  r refresh  L log  ? help"
        }
        Tab::Versions => "enter install  ↑↓ move  L log  ? help",
        Tab::Mods => "enter install  s search  x remove  esc back  L log  ? help",
        Tab::MegaHack => "enter install  x remove  c reload catalogue  L log  ? help",
        Tab::Steam => "enter launch  c copy in  l link  s sign in  r refresh  L log  ? help",
        Tab::Settings => "←→ change  enter edit  R reset  L log  ? help",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {keys}"),
            Style::new().fg(theme.muted),
        ))),
        area,
    );
}

fn draw_qr(frame: &mut Frame, app: &App, lines: &[String]) {
    let theme = app.theme;
    let widest = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0) as u16;
    let width = (widest + 6).min(frame.area().width.saturating_sub(2));
    let height = (lines.len() as u16 + 6).min(frame.area().height.saturating_sub(2));
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .title(Span::styled(
            " steam sign in ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let text: Vec<Line> = lines
        .iter()
        .map(|line| Line::from(Span::styled(line.clone(), Style::new().fg(theme.text))))
        .collect();
    frame.render_widget(Paragraph::new(text), chunks[0]);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " scan with the Steam mobile app   esc hides this",
            Style::new().fg(theme.muted),
        ))),
        chunks[1],
    );
}

fn draw_help(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    let area = centered(frame.area(), 78, 26);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .title(Span::styled(
            " keys ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut rows: Vec<(&str, String)> = vec![
        ("1..6, tab", "switch screens".into()),
        ("↑↓ / j k", "move the selection".into()),
        ("g / G", "top / bottom".into()),
        ("enter", "the main action on this screen".into()),
        ("q", "quit, asks when a job is running".into()),
        ("L", "show or hide the activity panel".into()),
        ("?", "this list".into()),
        ("", String::new()),
        ("instances", "enter launch, m mega hack, l loader, d remove".into()),
        ("versions", "enter install, asks the name first".into()),
        ("mods", "s search, enter install, x remove".into()),
        ("mega hack", "enter install, x remove, c reload catalogue".into()),
        ("steam", "enter launch, c copy in, l link, s sign in".into()),
        ("settings", "←→ change, enter edit, R reset".into()),
        ("themes", super::theme::names().join(", ")),
    ];
    let lines: Vec<Line> = rows
        .drain(..)
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!(" {key:<14}"), Style::new().fg(theme.accent)),
                Span::styled(what, Style::new().fg(theme.text)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_confirm(frame: &mut Frame, app: &App, confirm: &Confirm) {
    let theme = app.theme;
    let area = centered(frame.area(), 68, 10);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.warn))
        .title(Span::styled(
            format!(" {} ", confirm.title),
            Style::new().fg(theme.warn).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    frame.render_widget(
        Paragraph::new(confirm.body.clone())
            .style(Style::new().fg(theme.text))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    let buttons = Line::from(vec![
        Span::styled(
            format!(" y {} ", confirm.yes_label),
            Style::new().fg(theme.sel_fg).bg(theme.sel_bg),
        ),
        Span::raw("   "),
        Span::styled(
            format!(" n {} ", confirm.no_label),
            Style::new().fg(theme.text),
        ),
        Span::styled("   esc cancel", Style::new().fg(theme.muted)),
    ]);
    frame.render_widget(Paragraph::new(buttons), chunks[1]);
}

fn draw_input(frame: &mut Frame, app: &App, input: &Input) {
    let theme = app.theme;
    let area = centered(frame.area(), 74, 7);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .title(Span::styled(
            format!(" {} ", input.title),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            input.prompt.clone(),
            Style::new().fg(theme.muted),
        ))),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::new().fg(theme.accent)),
            Span::styled(input.value.clone(), Style::new().fg(theme.text)),
            Span::styled("█", Style::new().fg(theme.accent)),
        ])),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter accepts   esc cancels",
            Style::new().fg(theme.muted),
        ))),
        chunks[2],
    );
}

fn empty(frame: &mut Frame, app: &App, area: Rect, text: &str) {
    let theme = app.theme;
    frame.render_widget(
        Paragraph::new(text.to_string())
            .style(Style::new().fg(theme.muted))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn right(text: impl Into<String>) -> Cell<'static> {
    Cell::from(Text::from(text.into()).alignment(Alignment::Right))
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
