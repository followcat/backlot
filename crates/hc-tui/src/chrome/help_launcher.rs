//! The `Ctrl+B ?` launcher's painter (S5 §1): the same popup the
//! keybind reference draws — title `Actions & Keybinds`, a `/`
//! filter line, group headers over single-line rows — but each row
//! carries a right-aligned key hint and Enter acts. The two-line
//! glyph legend stays pinned above the footer (S0). Rows truncate
//! rather than wrap, so scrolling and row hits are line-exact —
//! `HelpRender::rows` feeds the mouse the row under each press.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::help::{self, HelpRender, HelpState};
use super::palette::Palette;
use super::render::put_text;
use super::scroll::{self, ScrollMetrics};
use super::text::{display_width, truncate_end};
use super::text_editor;
use crate::help_launcher::{HelpGroup, HelpPicker};

/// One draw line in the scroll window: a group header (`None`) or a
/// selectable row's flat index (`Some`).
pub(crate) fn draw_index(groups: &[HelpGroup]) -> Vec<Option<usize>> {
    let mut out = Vec::new();
    let mut row = 0;
    for group in groups {
        if group.rows.is_empty() {
            continue;
        }
        out.push(None);
        for _ in &group.rows {
            out.push(Some(row));
            row += 1;
        }
        out.push(None); // the blank line after each group
    }
    out
}

/// The overlay's frame — the same size ladder the keybind popup
/// follows (§3): 76x22 capped at the band minus four columns and two
/// rows.
pub(crate) fn render_launcher(
    b: &mut Buffer,
    area: Rect,
    h: &HelpState,
    groups: &[HelpGroup],
    legend: &[(String, String)],
    p: &Palette,
) -> Option<HelpRender> {
    let q = help::popup_with_width_cap(area, 76, 22, 4)?;
    let i = help::panel(b, q, p.accent, p.panel_bg)?;
    if i.width < 20 || i.height < 6 {
        return None;
    }
    put_text(
        b,
        i.x,
        i.y,
        i.width,
        "Actions & Keybinds",
        Style::default()
            .fg(p.text)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    let close = Rect::new(i.right() - 13, i.y, 13, 1);
    help::button(
        b,
        close,
        if h.search_focused {
            " esc back "
        } else {
            " esc close "
        },
        Style::default()
            .fg(help::contrast(p))
            .bg(p.accent)
            .add_modifier(Modifier::BOLD),
    );
    let sy = i.y + 1;
    put_text(
        b,
        i.x,
        sy,
        i.width,
        &if h.search_focused {
            " / ".to_owned()
        } else {
            " / press / to filter by command or shortcut".to_owned()
        },
        Style::default()
            .fg(if h.search_focused { p.text } else { p.overlay0 })
            .bg(p.panel_bg),
    );
    let cursor = if h.search_focused {
        text_editor::render(
            b,
            Rect::new(i.x + 3, sy, i.width.saturating_sub(3), 1),
            &h.query,
            Style::default().fg(p.text).bg(p.panel_bg),
        )
    } else {
        None
    };

    let legend_rows = u16::try_from(legend.len().min(2)).unwrap_or(0);
    let body = Rect::new(
        i.x,
        i.y + 3,
        i.width,
        i.height.saturating_sub(5 + legend_rows),
    );
    let index = draw_index(groups);
    let total = index.len();
    let viewport = usize::from(body.height.max(1));
    let max_scroll = total.saturating_sub(viewport);
    let scroll_rows = h.scroll.min(max_scroll);
    let metrics = ScrollMetrics {
        offset_from_bottom: max_scroll.saturating_sub(scroll_rows),
        max_offset_from_bottom: max_scroll,
        viewport_rows: viewport,
    };
    let needs_scrollbar = total > viewport;
    let text_area = if needs_scrollbar {
        Rect::new(body.x, body.y, body.width.saturating_sub(1), body.height)
    } else {
        body
    };
    let scrollbar = needs_scrollbar.then_some(Rect::new(
        body.right().saturating_sub(1),
        body.y,
        1,
        body.height,
    ));

    // Flatten the groups once more for painting: the draw index
    // names each line; the row hits record its flat row index.
    let mut hits = HelpRender {
        area: q,
        cancel: close,
        scrollbar: scrollbar.unwrap_or_default(),
        metrics: Some(metrics),
        max_scroll,
        cursor,
        ..HelpRender::default()
    };
    // Paint line by line: header, rows, blank — `pos` is the flat
    // draw position `scroll` skips, `flat` the selectable row index
    // a hit records.
    let mut flat = 0usize;
    let mut pos = 0usize;
    let mut y = body.y;
    'outer: for group in groups {
        if group.rows.is_empty() {
            continue;
        }
        if pos >= scroll_rows && y < body.bottom() {
            put_text(
                b,
                text_area.x,
                y,
                text_area.width,
                &format!(" {}", group.title),
                Style::default()
                    .fg(p.accent)
                    .bg(p.panel_bg)
                    .add_modifier(Modifier::BOLD),
            );
            y += 1;
        }
        pos += 1;
        for row in &group.rows {
            let row_idx = flat;
            flat += 1;
            if pos >= scroll_rows && y < body.bottom() {
                paint_row(b, text_area, y, row, row_idx == h.selected, p);
                hits.rows
                    .push((Rect::new(text_area.x, y, text_area.width, 1), row_idx));
                y += 1;
            }
            pos += 1;
        }
        // The trailing blank: occupies a draw line, paints nothing.
        if pos >= scroll_rows && y < body.bottom() {
            y += 1;
        }
        pos += 1;
        if y >= body.bottom() && pos > scroll_rows {
            break 'outer;
        }
    }

    if let Some(track) = scrollbar
        && let Some(thumb) = scroll::scrollbar_thumb(metrics, track)
    {
        for ty in track.y..track.bottom() {
            b[(track.x, ty)]
                .set_symbol("▐")
                .set_style(Style::default().fg(p.overlay0).bg(p.panel_bg));
        }
        for ty in thumb.top..thumb.top.saturating_add(thumb.len) {
            b[(track.x, ty)]
                .set_symbol("▐")
                .set_style(Style::default().fg(p.overlay1).bg(p.panel_bg));
        }
    }

    for (idx, (key, label)) in legend.iter().enumerate() {
        let ly = i.bottom() - 1 - legend_rows + idx as u16;
        put_text(
            b,
            i.x,
            ly,
            i.width,
            &format!(" {key:<7} {label}"),
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }

    put_text(
        b,
        i.x,
        i.bottom() - 1,
        i.width,
        if h.search_focused {
            " edit ←→/home/end · kill ^u/^k · yank ^y · back esc"
        } else {
            " ↑↓ move   ↵ run selected action   / filter   esc close"
        },
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    Some(hits)
}

/// One launcher row: `▌▸` and `selection_bg` when selected, the
/// glyph (waiting rows only), the label, then the hint right-aligned
/// in `mauve`.
fn paint_row(
    b: &mut Buffer,
    rect: Rect,
    y: u16,
    row: &crate::help_launcher::HelpRow,
    selected: bool,
    p: &Palette,
) {
    let bg = if selected { p.selection_bg } else { p.panel_bg };
    b.set_style(Rect::new(rect.x, y, rect.width, 1), Style::default().bg(bg));
    if selected {
        put_text(b, rect.x, y, 2, "▌▸", Style::default().fg(p.accent).bg(bg));
    }
    let label_x = rect.x + 3;
    let glyph_w = if row.glyph.is_empty() {
        0
    } else {
        put_text(
            b,
            label_x,
            y,
            1,
            row.glyph,
            Style::default().fg(p.text).bg(bg),
        );
        2
    };
    let hint_w = display_width(&row.hint) as u16;
    let hint_x = rect.right().saturating_sub(hint_w + 1);
    let label_w = hint_x.saturating_sub(label_x + glyph_w + 1);
    put_text(
        b,
        label_x + glyph_w,
        y,
        label_w,
        &truncate_end(&row.label, label_w as usize),
        Style::default().fg(p.text).bg(bg),
    );
    put_text(
        b,
        hint_x,
        y,
        hint_w,
        &truncate_end(&row.hint, hint_w as usize),
        Style::default().fg(p.mauve).bg(bg),
    );
}

/// The compact target picker — a floating list inside the overlay's
/// panel, named by the row that opened it. Returns the frame and
/// per-row rects through the `HelpRender` the caller already owns.
pub(crate) fn render_picker(
    b: &mut Buffer,
    inside: Rect,
    picker: &HelpPicker,
    hits: &mut HelpRender,
    p: &Palette,
) {
    let width = inside.width.min(48);
    let height = (picker.rows.len() as u16 + 4).min(inside.height);
    if width < 12 || height < 5 {
        return;
    }
    let q = Rect::new(
        inside.x + (inside.width - width) / 2,
        inside.y + (inside.height - height) / 2,
        width,
        height,
    );
    let Some(i) = help::panel(b, q, p.accent, p.panel_bg) else {
        return;
    };
    hits.picker = q;
    put_text(
        b,
        i.x,
        i.y,
        i.width,
        &truncate_end(&format!(" {}", picker.title), i.width as usize),
        Style::default()
            .fg(p.accent)
            .bg(p.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    let rows_h = i.height.saturating_sub(2);
    for (pos, row) in picker.rows.iter().enumerate().take(rows_h as usize) {
        let y = i.y + 1 + pos as u16;
        let rect = Rect::new(i.x, y, i.width, 1);
        let selected = pos == picker.selected;
        let bg = if selected { p.selection_bg } else { p.panel_bg };
        b.set_style(rect, Style::default().bg(bg));
        let text = if selected {
            format!("▸ {}", row.label)
        } else {
            format!("  {}", row.label)
        };
        put_text(
            b,
            i.x,
            y,
            i.width,
            &truncate_end(&text, i.width as usize),
            Style::default().fg(p.text).bg(bg),
        );
        hits.picker_rows.push((rect, pos));
    }
    put_text(
        b,
        i.x,
        i.bottom() - 1,
        i.width,
        " ↑↓ pick   ↵ run   esc back",
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
}
