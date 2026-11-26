//! Renderer for the unified [`HwSwitch`] modal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, Widget};

use super::{Field, HwHit, HwHits, HwSwitch};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_segment, put_text};
use crate::chrome::text::{display_width, truncate_end};
use crate::hollywood::wrap_plain;
use crate::new_studio::{ModelPick, Probe};

const REASSURANCE: &str = "make-before-break: old agent stays live until the new one is ready; brief + recent conversation carry over.";

fn frame(band: Rect, pick_open: bool, has_existing: bool) -> Option<Rect> {
    let w = band.width.saturating_sub(4).min(64);
    let base_h = if has_existing { 14 } else { 11 };
    let h = if pick_open { 16 } else { base_h }.min(band.height.saturating_sub(2));
    if w < 26 || h < 8 {
        return None;
    }
    Some(Rect::new(
        band.x + (band.width - w) / 2,
        band.y + (band.height - h) / 2,
        w,
        h,
    ))
}

/// Draw `HwSwitch` centered inside `band`, returning clickable rects and cursor.
pub(crate) fn render(b: &mut Buffer, d: &HwSwitch, p: &Palette, band: Rect) -> HwHits {
    let mut hits = HwHits::default();
    let Some(outer) = frame(band, d.model_pick.is_some(), d.existing.is_some()) else {
        put_text(
            b,
            b.area.x,
            b.area.y,
            b.area.width,
            "hollywood switch: terminal too small",
            Style::default().fg(p.red),
        );
        return hits;
    };
    Clear.render(outer, b);
    let Some(inner) = crate::chrome::dialogs::panel(b, outer, p.accent, p.panel_bg) else {
        return hits;
    };
    hits.area = outer;
    let title = if d.existing.is_some() {
        " Switch Hollywood's agent "
    } else {
        " Hollywood setup "
    };
    put_text(
        b,
        inner.x,
        inner.y,
        inner.width,
        title,
        Style::default()
            .fg(p.accent)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_right_text(
        b,
        inner,
        inner.y,
        "esc cancel",
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    let body = Rect::new(
        inner.x + 1,
        inner.y + 2,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(3),
    );
    if let Some(pick) = &d.model_pick {
        draw_pick(b, body, d, pick, p, &mut hits);
        return hits;
    }
    match &d.runners {
        Probe::Pending => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                "detecting runners…",
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Failed(e) => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                &truncate_end(e, body.width as usize),
                Style::default().fg(p.red).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Done(_) if d.found().is_empty() => {
            put_text(
                b,
                body.x,
                body.y,
                body.width,
                "no runners installed on this machine",
                Style::default().fg(p.peach).bg(p.panel_bg),
            );
            put_text(
                b,
                body.x,
                body.y + 1,
                body.width,
                "install one — `backlot runners` lists what is looked for",
                Style::default().fg(p.overlay0).bg(p.panel_bg),
            );
            return hits;
        }
        Probe::Done(_) => {}
    }

    let mut y = body.y;
    if let Some(cur) = d.current_label()
        && y < body.bottom().saturating_sub(1)
    {
        let mut label = cur;
        if d.prefer_alternative {
            label.push_str("  (alternative pre-selected)");
        }
        put_text(
            b,
            body.x + 1,
            y,
            body.width.saturating_sub(1),
            &truncate_end(&label, body.width.saturating_sub(1) as usize),
            Style::default().fg(p.subtext0).bg(p.panel_bg),
        );
        y += 1;
    }
    for field in d.fields() {
        if y >= body.bottom().saturating_sub(1) {
            break;
        }
        let line = Rect::new(body.x, y, body.width, 1);
        let selected = field == d.field;
        let bg = if selected { p.surface0 } else { p.panel_bg };
        if selected {
            b.set_style(line, Style::default().bg(p.surface0));
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(bg));
        }
        if field == Field::Apply {
            let label = if d.existing.is_some() {
                "switch agent"
            } else {
                "apply"
            };
            put_text(
                b,
                body.x + 1,
                y,
                body.width,
                label,
                Style::default()
                    .fg(if selected { p.accent } else { p.teal })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            );
            hits.push(line, HwHit::Field(field));
            y += 1;
            continue;
        }
        let (name, value) = match field {
            Field::Runner => (
                "runner",
                d.runner_row()
                    .map(|r| r.runner.clone())
                    .unwrap_or_else(|| "-".into()),
            ),
            Field::Model => ("model", d.model.clone()),
            Field::Effort => ("effort", d.effort_value().unwrap_or("-").to_string()),
            Field::Apply => ("apply", String::new()),
        };
        put_text(
            b,
            body.x + 1,
            y,
            8,
            name,
            Style::default().fg(p.overlay0).bg(bg),
        );
        let cell = Rect::new(body.x + 9, y, body.width.saturating_sub(9), 1);
        if selected && field == Field::Model && d.model_edit.is_some() {
            b.set_style(cell, Style::default().bg(p.surface0));
            if let Some(edit) = &d.model_edit
                && let Some(cursor) = crate::chrome::text_editor::render(
                    b,
                    cell,
                    edit,
                    Style::default().fg(p.text).bg(p.surface0),
                )
            {
                hits.cursor = Some(cursor);
            }
        } else {
            let (text, style) = if selected {
                (
                    format!("‹ {value} ›"),
                    Style::default()
                        .fg(p.accent)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (value, Style::default().fg(p.text).bg(bg))
            };
            put_segment(
                b,
                cell.x,
                y,
                cell.right(),
                &truncate_end(&text, cell.width as usize),
                style,
            );
        }
        hits.push(line, HwHit::Field(field));
        y += 1;
    }
    if let Some(err) = d.error.as_deref()
        && y < body.bottom().saturating_sub(1)
    {
        put_text(
            b,
            body.x + 1,
            y,
            body.width.saturating_sub(1),
            &truncate_end(err, body.width.saturating_sub(1) as usize),
            Style::default().fg(p.red).bg(p.panel_bg),
        );
        y += 1;
    }
    if d.existing.is_some() && y < body.bottom().saturating_sub(1) {
        let avail_w = body.width.saturating_sub(1) as usize;
        let max_lines = body.bottom().saturating_sub(1).saturating_sub(y).min(2) as usize;
        for line_txt in wrap_plain(REASSURANCE, avail_w.max(12), max_lines) {
            if y >= body.bottom().saturating_sub(1) {
                break;
            }
            put_text(
                b,
                body.x + 1,
                y,
                body.width.saturating_sub(1),
                &line_txt,
                Style::default().fg(p.teal).bg(p.panel_bg),
            );
            y += 1;
        }
    }
    let hint = crate::chrome::text::fit_hints(
        &[
            " ↑↓ field  ←→ value  ⏎ list · apply  e text  esc cancel",
            " ↑↓ field  ←→ value  ⏎ apply  esc",
            " ↑↓ · ←→ · ⏎ · esc",
        ],
        inner.width as usize,
    );
    put_text(
        b,
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        hint,
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    hits
}

fn draw_pick(
    b: &mut Buffer,
    body: Rect,
    d: &HwSwitch,
    pick: &ModelPick,
    p: &Palette,
    hits: &mut HwHits,
) {
    let runner = d.runner_row().map(|r| r.runner.as_str()).unwrap_or("-");
    let head_style = Style::default()
        .fg(p.accent)
        .bg(p.panel_bg)
        .add_modifier(Modifier::BOLD);
    let head = truncate_end(
        &format!("Model for hollywood · {runner}:"),
        body.width as usize,
    );
    let head_w = display_width(&head) as u16;
    let mut y = body.y;
    put_text(b, body.x, y, body.width, &head, head_style);
    let tail_x = body.x + head_w;
    if pick.filter.as_str().is_empty() {
        let tail_w = body.right().saturating_sub(tail_x);
        put_text(
            b,
            tail_x,
            y,
            tail_w,
            &truncate_end("  (type to filter)", tail_w as usize),
            head_style,
        );
    } else {
        let x = put_segment(b, tail_x, y, body.right(), "  /", head_style);
        let w = body.right().saturating_sub(x);
        if w > 0
            && let Some(cursor) = crate::chrome::text_editor::render(
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
        let bg = if selected { p.surface0 } else { p.panel_bg };
        if selected {
            b.set_style(line, Style::default().bg(p.surface0));
            put_text(b, body.x, y, 1, "▸", Style::default().fg(p.accent).bg(bg));
        }
        let text = if *option == d.model {
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
                truncate_end(&text, (body.width as usize).saturating_sub(3))
            ),
            Style::default().fg(p.text).bg(bg),
        );
        if *option == "auto" {
            put_segment(
                b,
                x,
                y,
                body.right(),
                " · runner default",
                Style::default().fg(p.overlay0).bg(bg),
            );
        }
        hits.push(line, HwHit::ModelPick(i));
        y += 1;
        if y >= body.bottom() {
            break;
        }
    }
}
