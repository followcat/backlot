//! Step 3 — the cast table. Runners are probed on the target
//! machine with `backlot runners --json`; each role row cycles its
//! runner, model and effort among what was detected. `a`/`x` add
//! and remove actor candidates; `^E` opens the generated cast.toml
//! in `$EDITOR` and reloads it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::step4::display_fits;
use super::{NewStudio, NsHit, NsHits, Probe, selected_row};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_segment, put_text};
use crate::chrome::text_editor;

/// The runner names `backlot runners` checks — the no-runners
/// banner lists what was looked for.
const RUNNER_NAMES: &str = "claude, codex, devin, cursor, opencode, cmd, cline";

/// The three value columns' (width, x-offset from body.x) — narrow
/// frames give the model column what is left.
fn columns(width: u16) -> [(u16, u16); 3] {
    let role = (width / 5).clamp(10, 20);
    let runner = (width / 7).clamp(7, 12);
    let effort = (width / 9).clamp(6, 9);
    let mut x = 2 + role + 1;
    let runner_x = x;
    x += runner + 1;
    let model_x = x;
    let model_w = width
        .saturating_sub(model_x)
        .saturating_sub(effort)
        .saturating_sub(1);
    x += model_w + 1;
    [(runner, runner_x), (model_w, model_x), (effort, x)]
}

fn role_col_width(width: u16) -> u16 {
    (width / 5).clamp(10, 20)
}

pub(crate) fn draw(b: &mut Buffer, body: Rect, d: &NewStudio, p: &Palette, hits: &mut NsHits) {
    b.set_style(body, Style::default().bg(p.panel_bg));
    let mut y = body.y;
    let machine = d.machine_label();

    // The detection line — or the no-runners banner.
    match &d.runners {
        Probe::Pending => {
            put_text(
                b,
                body.x,
                y,
                body.width,
                &format!(" Detecting runners on {machine}…"),
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
            return;
        }
        Probe::Failed(e) => {
            put_text(
                b,
                body.x,
                y,
                body.width,
                &crate::chrome::text::truncate_end(e, body.width as usize - 1),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
            return;
        }
        Probe::Done(runners) => {
            let found: Vec<String> = runners
                .iter()
                .filter(|r| r.found())
                .map(|r| match &r.version {
                    Some(v) => format!("{} {v}", r.runner),
                    None => r.runner.clone(),
                })
                .collect();
            if found.is_empty() {
                put_text(
                    b,
                    body.x,
                    y,
                    body.width,
                    &crate::chrome::text::truncate_end(
                        &format!(" No runners found on {machine} — checked {RUNNER_NAMES}"),
                        body.width as usize - 1,
                    ),
                    Style::default().fg(p.peach).bg(p.panel_bg),
                );
                put_text(
                    b,
                    body.x,
                    y + 1,
                    body.width,
                    " Install a runner there — the stage needs one to seat roles",
                    Style::default().fg(p.overlay0).bg(p.panel_bg),
                );
                return;
            }
            let head = format!(" Detected runners on {machine}: {}", found.join(", "));
            put_text(
                b,
                body.x,
                y,
                body.width,
                &crate::chrome::text::truncate_end(&head, body.width as usize - 1),
                Style::default().fg(p.text).bg(p.panel_bg),
            );
            // The ^E hint rides the right edge only while it clears
            // the head — at narrow widths the head keeps the room.
            let edit_hint = "(^E open $EDITOR)";
            if display_fits(body.width, &head, edit_hint) {
                put_text(
                    b,
                    body.right().saturating_sub(edit_hint.len() as u16),
                    y,
                    edit_hint.len() as u16,
                    edit_hint,
                    Style::default().fg(p.overlay0).bg(p.panel_bg),
                );
            }
        }
    }
    y += 2;
    if y >= body.bottom() {
        return;
    }

    // The role table header.
    let header = Style::default()
        .fg(p.overlay0)
        .bg(p.panel_bg)
        .add_modifier(Modifier::BOLD);
    let cols = columns(body.width);
    let role_w = role_col_width(body.width);
    put_segment(b, body.x + 2, y, body.right(), "ROLE / CANDIDATE", header);
    for (title, (_, off)) in [("RUNNER", cols[0]), ("MODEL", cols[1]), ("EFFORT", cols[2])] {
        put_segment(b, body.x + off, y, body.right(), title, header);
    }
    y += 1;

    let runners = match &d.runners {
        Probe::Done(r) => r.as_slice(),
        _ => &[],
    };
    for (i, row) in d.cast.rows.iter().enumerate() {
        if y >= body.bottom().saturating_sub(1) {
            break;
        }
        let line = Rect::new(body.x, y, body.width, 1);
        let selected = i == d.cast.sel;
        if selected {
            selected_row(b, line, p);
            put_text(
                b,
                body.x,
                y,
                1,
                "▸",
                Style::default().fg(p.accent).bg(p.panel_bg),
            );
        }
        let role = if row.candidate {
            format!("{} (cand)", row.key)
        } else {
            row.key.clone()
        };
        put_segment(
            b,
            body.x + 2,
            y,
            (body.x + 2 + role_w).min(body.right()),
            &role,
            Style::default().fg(p.teal).bg(p.panel_bg),
        );
        hits.push(
            Rect::new(body.x + 2, y, role_w.min(body.width.saturating_sub(2)), 1),
            NsHit::CastCell { row: i, col: 0 },
        );
        let runner = runners.get(row.runner);
        let cells = [
            runner.map(|r| r.runner.as_str()).unwrap_or("-").to_string(),
            runner
                .and_then(|r| r.models.get(row.model).cloned())
                .unwrap_or_else(|| "-".into()),
            runner
                .and_then(|r| r.efforts.get(row.effort).cloned())
                .unwrap_or_else(|| "-".into()),
        ];
        for (col, (w, x)) in cols.iter().enumerate() {
            let cell = Rect::new(
                body.x + *x,
                y,
                (*w).min(body.right().saturating_sub(body.x + *x)),
                1,
            );
            if cell.width == 0 {
                continue;
            }
            let active = selected && d.cast.col == col;
            // `e` turned this model cell into a free-text field.
            if active && col == 1 && d.model_edit.is_some() {
                b.set_style(cell, Style::default().bg(p.surface0));
                if let Some(edit) = &d.model_edit
                    && let Some(cursor) = text_editor::render(
                        b,
                        cell,
                        edit,
                        Style::default().fg(p.text).bg(p.surface0),
                    )
                {
                    hits.cursor = Some(cursor);
                }
            } else {
                let text = cells[col].clone();
                let (text, style) = if active {
                    (
                        format!("‹ {text} ›"),
                        Style::default()
                            .fg(p.accent)
                            .bg(p.panel_bg)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    (text, Style::default().fg(p.text).bg(p.panel_bg))
                };
                put_segment(b, cell.x, y, cell.right(), &text, style);
            }
            hits.push(cell, NsHit::CastCell { row: i, col });
        }
        y += 1;
    }
    y += 1;
    if y < body.bottom() {
        put_text(
            b,
            body.x + 2,
            y,
            body.width.saturating_sub(2),
            " + [a] add candidate   [x] remove   rework cap: 2",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        // A hand-edited cast's error line sits right under it.
        if let Some(error) = &d.cast.custom_error
            && y + 1 < body.bottom()
        {
            put_text(
                b,
                body.x + 2,
                y + 1,
                body.width.saturating_sub(2),
                &crate::chrome::text::truncate_end(error, body.width as usize - 3),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
        }
    }
}
