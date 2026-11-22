//! Step 3 — the cast table. Runners are probed on the target
//! machine with `backlot runners --json`; each role row cycles its
//! runner, model and effort among what was detected. `a`/`x` add
//! and remove actor candidates; `^E` opens the generated cast.toml
//! in `$EDITOR` and reloads it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::step4::display_fits;
use super::{NewStudio, NsHit, NsHits, Probe, RunnerRow, selected_row};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_segment, put_text};
use crate::chrome::text_editor;

/// The runner names `backlot runners` checks — the no-runners
/// banner lists what was looked for.
const RUNNER_NAMES: &str = "claude, codex, devin, cursor, opencode, cmd, cline, agy";

/// A runner's version often repeats its own name (`claude 2.1.0`,
/// `fake-runner 1.0`). Strip that leading token so the detection
/// banner reads `claude 2.1.0, fake 1.0` without a stutter.
pub(crate) fn version_label(runner: &str, version: &str) -> String {
    /// `value` without `prefix` when it leads with it as a whole
    /// word — case-insensitively; byte offsets stay on char
    /// boundaries because `get` refuses a split.
    fn strip<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
        let head = value.get(..prefix.len())?;
        if !head.eq_ignore_ascii_case(prefix) {
            return None;
        }
        let rest = &value[prefix.len()..];
        if rest.is_empty() || rest.starts_with([' ', '\t']) {
            Some(rest.trim_start())
        } else {
            None
        }
    }
    let rest = strip(version, &format!("{runner}-runner"))
        .or_else(|| strip(version, runner))
        .unwrap_or(version)
        .trim();
    if rest.is_empty() {
        runner.to_string()
    } else {
        format!("{runner} {rest}")
    }
}

/// The three value columns' (width, x-offset from body.x) — narrow
/// frames give the model column what is left.
fn columns(width: u16) -> [(u16, u16); 3] {
    let role = (width / 5).clamp(10, 20);
    // The longest runner names the tool ships (`opencode`, `cline`,
    // `claude`) fit without an ellipsis once `‹ ›` is gone.
    let runner = (width / 7).clamp(9, 12);
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

/// The Cast footer's tier list for the focused cell: `space` opens the
/// MODEL column's catalog (RUNNER and EFFORT take their next value),
/// `←`/`→` change the cell's value, and `⏎` runs the Step-4 dry run
/// from every column — the spec's one studio key map.
pub(crate) fn hint_tiers(col: usize) -> &'static [&'static str] {
    match col {
        1 => &[
            "tab col  ↑↓ role  space pick  ←→ change  ⏎ dry-run  a add  x remove  ^E editor  esc back",
            "tab col  ↑↓  space pick  ←→ change  ⏎ dry-run  a/x  ^E  esc",
            "tab  space pick  ⏎ dry-run  esc",
        ],
        _ => &[
            "tab col  ↑↓ role  space next  ←→ change  ⏎ dry-run  a add  x remove  ^E editor  esc back",
            "tab col  ↑↓  space next  ←→ change  ⏎ dry-run  a/x  ^E  esc",
            "tab  space next  ⏎ dry-run  esc",
        ],
    }
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
                &crate::chrome::text::truncate_end(
                    &format!(" Detecting runners on {machine}…"),
                    body.width as usize,
                ),
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
                .map(|r| {
                    let base = match &r.version {
                        Some(v) => version_label(&r.runner, v),
                        None => r.runner.clone(),
                    };
                    match r.bin_hint() {
                        Some(dir) => format!("{base} · {dir}"),
                        None => base,
                    }
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

    // An open MODEL list owns the body until `⏎` seats or `esc`
    // drops it — the table is behind it.
    if let Some(pick) = &d.model_pick {
        draw_model_pick(b, body, y, d, pick, p, hits);
        return;
    }

    // The role table header.
    let header = Style::default()
        .fg(p.overlay0)
        .bg(p.panel_bg)
        .add_modifier(Modifier::BOLD);
    let cols = columns(body.width);
    let role_w = role_col_width(body.width);
    // Each header stays inside its column span, with the one-space
    // gap the rows use — the long label degrades to `ROLE` when it
    // cannot fit, so it never runs into `RUNNER`.
    let role_head_w = role_w as usize;
    put_text(
        b,
        body.x + 2,
        y,
        role_head_w as u16,
        crate::chrome::text::fit_hints(&["ROLE / CANDIDATE", "ROLE"], role_head_w),
        header,
    );
    let spans = [
        ("RUNNER", cols[0].1, cols[1].1),
        ("MODEL", cols[1].1, cols[2].1),
        ("EFFORT", cols[2].1, body.width),
    ];
    for (title, off, end) in spans {
        let w = end.saturating_sub(off);
        put_text(
            b,
            body.x + off,
            y,
            w,
            &crate::chrome::text::truncate_end(title, w as usize),
            header,
        );
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
        let rbg = super::row_text_bg(selected, p);
        if selected {
            selected_row(b, line, p);
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(rbg));
        }
        let role = if row.candidate {
            format!("{}*", row.key)
        } else {
            row.key.clone()
        };
        put_segment(
            b,
            body.x + 2,
            y,
            (body.x + 2 + role_w).min(body.right()),
            &crate::chrome::text::truncate_end(&role, role_w.saturating_sub(1) as usize),
            Style::default().fg(p.teal).bg(rbg),
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
                .and_then(|r| {
                    r.efforts.get(row.effort).cloned().or_else(|| {
                        // A runner with no effort flag shows the
                        // depth its model id carries.
                        if r.efforts.is_empty() {
                            r.models
                                .get(row.model)
                                .and_then(|m| super::model::depth_in(m))
                                .map(str::to_string)
                        } else {
                            None
                        }
                    })
                })
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
                // The focused cell is already marked by `▸`, the
                // selection background and bold — `‹ ›` would only
                // eat width a real runner or model name needs.
                let (text, style) = if active {
                    (
                        text,
                        Style::default()
                            .fg(p.accent)
                            .bg(rbg)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    (text, Style::default().fg(p.text).bg(rbg))
                };
                put_segment(
                    b,
                    cell.x,
                    y,
                    cell.right(),
                    &crate::chrome::text::truncate_end(&text, cell.width as usize),
                    style,
                );
            }
            hits.push(cell, NsHit::CastCell { row: i, col });
        }
        y += 1;
    }
    y += 1;
    // The focused row's whole value, under the table: a long model
    // name may clip in its column, and the runner's resolved binary
    // is what a launch will actually run. The path drops to its own
    // line rather than sharing the values line's remaining width.
    if y < body.bottom()
        && let Some((values, path)) = detail_line(d, runners)
    {
        let room = body.width.saturating_sub(3) as usize;
        put_text(
            b,
            body.x + 2,
            y,
            body.width.saturating_sub(2),
            &crate::chrome::text::truncate_end(&values, room),
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        y += 1;
        if let Some(path) = path
            && y < body.bottom()
        {
            put_text(
                b,
                body.x + 4,
                y,
                body.width.saturating_sub(4),
                &crate::chrome::text::truncate_end(&path, room),
                Style::default().fg(p.overlay1).bg(p.panel_bg),
            );
            y += 1;
        }
    }
    if y < body.bottom() {
        put_text(
            b,
            body.x + 2,
            y,
            body.width.saturating_sub(2),
            &crate::chrome::text::truncate_end(
                " + [a] add candidate   [x] remove   rework cap: 2",
                body.width.saturating_sub(2) as usize,
            ),
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

/// `role · runner · model · effort` for the focused row, and the
/// runner's resolved binary when detection named one — the lines that
/// keep a clipped column readable.
fn detail_line(d: &NewStudio, runners: &[RunnerRow]) -> Option<(String, Option<String>)> {
    let row = d.cast.rows.get(d.cast.sel)?;
    let runner = runners.get(row.runner);
    let name = runner.map(|r| r.runner.as_str()).unwrap_or("-");
    let model = runner
        .and_then(|r| r.models.get(row.model))
        .map(String::as_str)
        .unwrap_or("-");
    let effort = runner
        .and_then(|r| {
            r.efforts.get(row.effort).cloned().or_else(|| {
                if r.efforts.is_empty() {
                    r.models
                        .get(row.model)
                        .and_then(|m| super::model::depth_in(m))
                        .map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "-".into());
    let role = if row.candidate {
        format!("{}*", row.key)
    } else {
        row.key.clone()
    };
    let values = format!("{role} · {name} · {model} · {effort}");
    Some((values, runner.and_then(|r| r.path.clone())))
}

/// The open MODEL list: `auto` first, then the row's runner
/// catalog — the heading carries the live filter like the step-1
/// repository list, the current value wears the table's `‹ ›`
/// marks, and the window follows the selection.
fn draw_model_pick(
    b: &mut Buffer,
    body: Rect,
    mut y: u16,
    d: &NewStudio,
    pick: &super::ModelPick,
    p: &Palette,
    hits: &mut NsHits,
) {
    let runners = match &d.runners {
        Probe::Done(r) => r.as_slice(),
        _ => &[],
    };
    let Some(row) = d.cast.rows.get(d.cast.sel) else {
        return;
    };
    let runner = runners.get(row.runner);
    let runner_name = runner.map(|r| r.runner.as_str()).unwrap_or("-");
    let current = runner
        .and_then(|r| r.models.get(row.model))
        .map(String::as_str)
        .unwrap_or_default();
    let head_style = Style::default()
        .fg(p.accent)
        .bg(p.panel_bg)
        .add_modifier(Modifier::BOLD);
    let head = crate::chrome::text::truncate_end(
        &format!(" Model for {} · {runner_name}:", row.key),
        body.width as usize,
    );
    let head_w = crate::chrome::text::display_width(&head) as u16;
    put_text(b, body.x, y, body.width, &head, head_style);
    // The filter is always live — untouched the tail is the hint,
    // the first printable char turns it into the `/…` editor.
    let tail_x = body.x + head_w;
    if pick.filter.as_str().is_empty() {
        let tail_w = body.right().saturating_sub(tail_x);
        put_text(
            b,
            tail_x,
            y,
            tail_w,
            &crate::chrome::text::truncate_end("  (type to filter)", tail_w as usize),
            head_style,
        );
    } else {
        let x = put_segment(b, tail_x, y, body.right(), "  /", head_style);
        let w = body.right().saturating_sub(x);
        if w > 0
            && let Some(cursor) = text_editor::render(
                b,
                Rect::new(x, y, w, 1),
                &pick.filter,
                Style::default().fg(p.text).bg(p.panel_bg),
            )
        {
            hits.cursor = Some(cursor);
        }
    }
    y += 1;
    let visible = pick.visible();
    if visible.is_empty() {
        put_text(
            b,
            body.x + 1,
            y,
            body.width,
            "nothing matches the filter",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
        return;
    }
    // Keep the selection inside the window — the repo picker's own
    // scroll rule.
    let window = body.bottom().saturating_sub(y) as usize;
    if window == 0 {
        return;
    }
    let scroll = pick
        .scroll
        .min(visible.len().saturating_sub(window))
        .min(pick.sel)
        .max(pick.sel.saturating_sub(window.saturating_sub(1)));
    for (vi, option) in visible.iter().skip(scroll).take(window).enumerate() {
        let i = scroll + vi;
        let line = Rect::new(body.x, y, body.width, 1);
        let selected = i == pick.sel;
        let rbg = super::row_text_bg(selected, p);
        if selected {
            super::selected_row(b, line, p);
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(rbg));
        }
        let text = if *option == current {
            format!("‹ {option} ›")
        } else {
            (*option).to_string()
        };
        let x = put_segment(
            b,
            body.x + 2,
            y,
            body.right(),
            &format!(
                " {}",
                crate::chrome::text::truncate_end(&text, body.width as usize - 3)
            ),
            Style::default().fg(p.text).bg(rbg),
        );
        // `auto`'s meaning — the runner's own default — in a dim
        // note at the line's end.
        if *option == "auto" {
            put_segment(
                b,
                x,
                y,
                body.right(),
                " · runner default",
                Style::default().fg(p.overlay0).bg(rbg),
            );
        }
        hits.push(line, NsHit::ModelPick(i));
        y += 1;
        if y >= body.bottom() {
            break;
        }
    }
}
