//! Step 1 — target machine and repository. The machine column is
//! `Local` plus every attached machine with its reachability and
//! `backlot --version`; the repository column lists the target's
//! directories through `backlot repos --json` — every path on the
//! selected machine, never the local disk.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{MachineState, NewStudio, NsHit, NsHits, Probe, RepoKind, Step1Pane, selected_row};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_segment, put_text};

/// `●` usable, `○` unusable-but-online, `×` offline — the panel's
/// own dot vocabulary.
fn machine_dot(m: &super::MachineRow, p: &Palette) -> (&'static str, Style) {
    match &m.state {
        MachineState::Offline(_) => ("×", Style::default().fg(p.red)),
        MachineState::Online => match &m.backlot {
            Probe::Done(_) => ("●", Style::default().fg(p.green)),
            Probe::Pending => ("○", Style::default().fg(p.yellow)),
            Probe::Failed(_) => ("○", Style::default().fg(p.overlay0)),
        },
    }
}

/// The machine row's reason text — version, or why Enter is off.
fn machine_detail(m: &super::MachineRow) -> (String, bool) {
    match &m.state {
        MachineState::Offline(why) => (format!("offline · {why}"), false),
        MachineState::Online => match &m.backlot {
            Probe::Done(version) => (format!("online · backlot {version}"), true),
            Probe::Pending => ("online · checking for backlot…".into(), false),
            Probe::Failed(e) => (format!("online · {e}"), false),
        },
    }
}

/// A repo row's right-side note — kind, branch, and the in-use
/// warning or the reason it cannot be picked.
fn repo_note(e: &super::RepoEntry) -> (String, bool) {
    let mut note = match e.kind {
        RepoKind::GitReady => {
            let mut s = "git repo".to_string();
            if let Some(branch) = &e.branch {
                s.push_str(&format!(" · {branch}"));
            }
            s
        }
        RepoKind::GitEmpty => "git repo · 0 commits (commit first)".into(),
        RepoKind::Dir => "directory".into(),
    };
    if let Some(studio) = &e.in_use_studio {
        note.push_str(&format!(" · in use by studio {studio}"));
    }
    (note, e.kind != RepoKind::GitEmpty)
}

fn repo_glyph(e: &super::RepoEntry, p: &Palette) -> (&'static str, Style) {
    match e.kind {
        RepoKind::GitReady => {
            if e.in_use_studio.is_some() {
                ("●", Style::default().fg(p.peach))
            } else {
                ("●", Style::default().fg(p.mauve))
            }
        }
        RepoKind::GitEmpty => ("○", Style::default().fg(p.overlay0)),
        RepoKind::Dir => ("▸", Style::default().fg(p.teal)),
    }
}

pub(crate) fn draw(b: &mut Buffer, body: Rect, d: &NewStudio, p: &Palette, hits: &mut NsHits) {
    let bg = Style::default().bg(p.panel_bg);
    b.set_style(body, bg);
    let mut y = body.y;

    // — Machines —
    let label_style = Style::default().fg(p.text).bg(p.panel_bg);
    put_text(
        b,
        body.x,
        y,
        body.width,
        "Target machine (tab to switch):",
        label_style,
    );
    put_right_text(
        b,
        body,
        y,
        if d.refreshing {
            "^R re-check …"
        } else {
            "^R re-check"
        },
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    y += 1;
    let machine_rows = d.machines.len().min(5);
    for (i, m) in d.machines.iter().take(machine_rows).enumerate() {
        let row = Rect::new(body.x, y, body.width, 1);
        let selected = i == d.machine_sel;
        if selected && d.pane == Step1Pane::Machines {
            selected_row(b, row, p);
        }
        let (dot, dot_style) = machine_dot(m, p);
        let name_style = if m.usable() {
            Style::default().fg(p.text).bg(p.panel_bg)
        } else {
            Style::default().fg(p.overlay0).bg(p.panel_bg)
        };
        let x = put_segment(b, row.x + 1, y, row.right(), dot, dot_style.bg(p.panel_bg));
        let name = format!(" {:<12}", m.label);
        let x = put_segment(b, x, y, row.right().min(x + 14), &name, name_style);
        let (detail, ok) = machine_detail(m);
        let detail_style = if ok {
            Style::default().fg(p.subtext0).bg(p.panel_bg)
        } else {
            Style::default().fg(p.peach).bg(p.panel_bg)
        };
        put_text(
            b,
            x,
            y,
            row.right().saturating_sub(x),
            &detail,
            detail_style,
        );
        if selected {
            put_text(
                b,
                row.x,
                y,
                1,
                "▸",
                Style::default().fg(p.accent).bg(p.panel_bg),
            );
        }
        hits.push(row, NsHit::Machine(i));
        y += 1;
    }
    y += 1;

    // — Repositories —
    if y >= body.bottom() {
        return;
    }
    let machine = d.machine_label();
    let dir = d.repos.dir.as_deref().unwrap_or("…");
    let head = format!("Repository on {machine}:  {dir}  (/ filter)");
    put_text(
        b,
        body.x,
        y,
        body.width,
        &format!(" {head}"),
        Style::default()
            .fg(p.accent)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    y += 1;
    let list_h = body.bottom().saturating_sub(y);
    if list_h == 0 {
        return;
    }
    match &d.repos.listing {
        Probe::Pending => {
            put_text(
                b,
                body.x + 1,
                y,
                body.width,
                "listing repositories…",
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
        }
        Probe::Failed(e) => {
            put_text(
                b,
                body.x + 1,
                y,
                body.width.saturating_sub(1),
                &truncate(e, body.width as usize - 2),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
        }
        Probe::Done(_) => {
            let visible = d.repos.visible();
            if visible.is_empty() {
                put_text(
                    b,
                    body.x + 1,
                    y,
                    body.width,
                    if d.repos.filter.as_str().is_empty() {
                        "empty directory"
                    } else {
                        "nothing matches the filter"
                    },
                    Style::default().fg(p.overlay0).bg(p.panel_bg),
                );
            }
            // Keep the selection inside the window: scroll so `sel`
            // is always painted.
            let window = list_h as usize;
            let scroll = d
                .repos
                .scroll
                .min(visible.len().saturating_sub(window))
                .min(d.repos.sel)
                .max(d.repos.sel.saturating_sub(window.saturating_sub(1)));
            for (vi, e) in visible.iter().skip(scroll).take(window).enumerate() {
                let i = scroll + vi;
                let row = Rect::new(body.x, y, body.width, 1);
                let selected = i == d.repos.sel;
                if selected && d.pane == Step1Pane::Repos {
                    selected_row(b, row, p);
                }
                let (glyph, glyph_style) = repo_glyph(e, p);
                let mut x = put_segment(
                    b,
                    row.x + 1,
                    y,
                    row.right(),
                    glyph,
                    glyph_style.bg(p.panel_bg),
                );
                let name_style = if e.kind == RepoKind::GitEmpty {
                    Style::default().fg(p.overlay0).bg(p.panel_bg)
                } else {
                    Style::default().fg(p.text).bg(p.panel_bg)
                };
                let name = if e.kind == RepoKind::Dir {
                    format!(" {}/", e.name)
                } else {
                    format!(" {}", e.name)
                };
                let name_w = 20u16.min(body.width / 3).max(10);
                x = put_segment(
                    b,
                    x,
                    y,
                    row.right().min(x + name_w),
                    &format!("{name:<w$}", w = name_w as usize - 2),
                    name_style,
                );
                let (note, ok) = repo_note(e);
                let note_style = if e.in_use_studio.is_some() {
                    Style::default().fg(p.peach).bg(p.panel_bg)
                } else if ok {
                    Style::default().fg(p.subtext0).bg(p.panel_bg)
                } else {
                    Style::default().fg(p.red).bg(p.panel_bg)
                };
                put_text(b, x, y, row.right().saturating_sub(x), &note, note_style);
                if selected {
                    put_text(
                        b,
                        row.x,
                        y,
                        1,
                        "▸",
                        Style::default().fg(p.accent).bg(p.panel_bg),
                    );
                }
                hits.push(row, NsHit::Repo(i));
                y += 1;
                if y >= body.bottom() {
                    break;
                }
            }
        }
    }
}

fn truncate(s: &str, w: usize) -> String {
    crate::chrome::text::truncate_end(s, w)
}
