//! Step 2 — the studio id and title on the chosen machine. The id
//! validates live against `[a-z0-9][a-z0-9_-]*` and the machine's
//! existing studios; the title rides along.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::{NewStudio, NsHit, NsHits, Step2Field, id_error};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_text};
use crate::chrome::text_editor;

pub(crate) fn draw(b: &mut Buffer, body: Rect, d: &NewStudio, p: &Palette, hits: &mut NsHits) {
    b.set_style(body, Style::default().bg(p.panel_bg));
    let label = Style::default().fg(p.text).bg(p.panel_bg);
    let dim = Style::default().fg(p.overlay0).bg(p.panel_bg);
    let mut y = body.y;

    // The chosen target — machine and repo in one line.
    let machine = d.machine_label();
    let repo = d.repo();
    let repo_path = repo.map(|r| r.path.as_str()).unwrap_or_default();
    let branch = repo.and_then(|r| r.branch.as_deref());
    let target = match branch {
        Some(branch) => format!(" Target: {machine} · {repo_path} (branch: {branch})"),
        None => format!(" Target: {machine} · {repo_path}"),
    };
    put_text(
        b,
        body.x,
        y,
        body.width,
        &crate::chrome::text::truncate_end(&target, body.width as usize),
        label,
    );
    y += 2;

    // Studio ID — the slug, with the live verdict on the right.
    let head = format!(" Studio ID   (slug on {machine}; unique across {machine} studios)");
    put_text(
        b,
        body.x,
        y,
        body.width,
        &crate::chrome::text::truncate_end(&head, body.width as usize),
        label,
    );
    y += 1;
    if y >= body.bottom() {
        return;
    }
    let field = Rect::new(body.x, y, body.width, 1);
    let focused = d.field == Step2Field::Id;
    let style = Style::default()
        .fg(p.text)
        .bg(if focused { p.surface0 } else { p.panel_bg });
    b.set_style(field, style);
    let verdict = match id_error(d.id.as_str(), &d.studios) {
        Ok(()) => (
            "✓ available".to_string(),
            Style::default().fg(p.green).bg(p.panel_bg),
        ),
        Err(e) => (format!("× {e}"), Style::default().fg(p.red).bg(p.panel_bg)),
    };
    // The verdict rides the field's right edge when a usable field
    // remains; at narrow widths it drops to its own line instead of
    // painting over the text.
    let verdict_w = crate::chrome::text::display_width(&verdict.0) as u16;
    let inline = body.width.saturating_sub(verdict_w + 2) >= 16;
    let text_w = if inline {
        field.width.saturating_sub(verdict_w + 4)
    } else {
        field.width.saturating_sub(2)
    };
    if let Some(cursor) =
        text_editor::render(b, Rect::new(field.x + 1, field.y, text_w, 1), &d.id, style)
        && focused
    {
        hits.cursor = Some(cursor);
    }
    if inline {
        put_right_text(b, field, field.y, &verdict.0, verdict.1);
    } else if y + 1 < body.bottom() {
        put_text(
            b,
            body.x + 1,
            y + 1,
            body.width.saturating_sub(1),
            &crate::chrome::text::truncate_end(&verdict.0, body.width.saturating_sub(1) as usize),
            verdict.1,
        );
        y += 1;
    }
    hits.push(field, NsHit::IdField);
    y += 2;

    // Title / Requirement Headline.
    put_text(
        b,
        body.x,
        y,
        body.width,
        " Title / Requirement Headline",
        label,
    );
    y += 1;
    if y >= body.bottom() {
        return;
    }
    let field = Rect::new(body.x, y, body.width, 1);
    let focused = d.field == Step2Field::Title;
    let style = Style::default()
        .fg(p.text)
        .bg(if focused { p.surface0 } else { p.panel_bg });
    b.set_style(field, style);
    if let Some(cursor) = text_editor::render(
        b,
        Rect::new(field.x + 1, field.y, field.width.saturating_sub(2), 1),
        &d.title,
        style,
    ) && focused
    {
        hits.cursor = Some(cursor);
    }
    hits.push(field, NsHit::TitleField);
    y += 2;

    // Mode (R14): a two-value toggle — `←`/`→`/space or a click
    // flips it; the focused field takes the selection surface like
    // the text fields do.
    if y >= body.bottom() {
        return;
    }
    put_text(b, body.x, y, body.width, " Mode", label);
    y += 1;
    if y >= body.bottom() {
        return;
    }
    let field = Rect::new(body.x, y, body.width, 1);
    let focused = d.field == Step2Field::Mode;
    let bg = if focused { p.surface0 } else { p.panel_bg };
    b.set_style(field, Style::default().fg(p.text).bg(bg));
    let experimental = d.mode == backlot_model::event::StudioMode::Experimental;
    let on = Style::default().fg(p.accent).bg(bg);
    let off = Style::default().fg(p.overlay0).bg(bg);
    let (s_style, e_style) = if experimental { (off, on) } else { (on, off) };
    let mut x = field.x + 1;
    for (word, style) in [("standard", s_style), ("|", off), ("experimental", e_style)] {
        put_text(b, x, y, field.right().saturating_sub(x), word, style);
        x += crate::chrome::text::display_width(word) as u16 + 1;
        if x >= field.right() {
            break;
        }
    }
    hits.push(field, NsHit::ModeField);
    y += 2;

    // What the mode means — one dim line under the toggle.
    if y < body.bottom() {
        let note = match experimental {
            true => " Experimental: hypotheses, competing variants, honest negatives",
            false => " Standard: the cast model's usual production flow",
        };
        put_text(
            b,
            body.x,
            y,
            body.width,
            &crate::chrome::text::truncate_end(note, body.width as usize),
            dim,
        );
        y += 2;
    }

    draw_build(b, body, y, d, p, hits);
}

/// The optional multi-line "What should this studio build?" brief —
/// heading, its editable body rows (as many as fit), and the `^E`
/// hint. Empty is the default: the producer asks first, as before.
fn draw_build(
    b: &mut Buffer,
    body: Rect,
    mut y: u16,
    d: &NewStudio,
    p: &Palette,
    hits: &mut NsHits,
) {
    let label = Style::default().fg(p.text).bg(p.panel_bg);
    let dim = Style::default().fg(p.overlay0).bg(p.panel_bg);
    if y >= body.bottom() {
        return;
    }
    put_text(
        b,
        body.x,
        y,
        body.width,
        " What should this studio build? (optional)",
        label,
    );
    y += 1;
    if y >= body.bottom() {
        return;
    }
    // The body rows: as many as fit below the heading, one row
    // minimum. The focused field shows the surface and the cursor.
    let rows = body.bottom().saturating_sub(y).clamp(1, 4);
    let area = Rect::new(body.x, y, body.width, rows);
    let focused = d.field == Step2Field::Build;
    let bg = if focused { p.surface0 } else { p.panel_bg };
    b.set_style(area, Style::default().bg(bg));
    let scroll = d
        .build
        .row
        .saturating_sub(usize::from(rows).saturating_sub(1));
    for (n, line_idx) in (scroll..).take(usize::from(rows)).enumerate() {
        let Some(row) = d.build.lines.get(line_idx) else {
            break;
        };
        let text: String = row.iter().collect();
        let line_y = y + n as u16;
        put_text(
            b,
            body.x + 1,
            line_y,
            body.width.saturating_sub(2),
            &crate::chrome::text::truncate_end(&text, body.width.saturating_sub(2) as usize),
            Style::default()
                .fg(if focused { p.text } else { p.overlay1 })
                .bg(bg),
        );
        if focused && line_idx == d.build.row {
            let prefix: String = row[..d.build.col.min(row.len())].iter().collect();
            let room = body.width.saturating_sub(2);
            let cx = body.x + 1 + (crate::chrome::text::display_width(&prefix) as u16).min(room);
            hits.cursor = Some(crate::chrome::help::CursorState {
                x: cx,
                y: line_y,
                visible: true,
            });
        }
    }
    hits.push(area, NsHit::BuildField);
    y += rows;
    if y < body.bottom() {
        let hint = if d.build_text().is_empty() {
            " empty — the producer will ask; alt+⏎ add a line, ^E editor, tab next"
        } else {
            " ^E editor, alt+⏎ add a line, tab next"
        };
        put_text(
            b,
            body.x,
            y,
            body.width,
            &crate::chrome::text::truncate_end(hint, body.width as usize),
            dim,
        );
    }
}
