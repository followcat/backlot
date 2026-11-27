//! The launcher's compact target picker (S5 §1): a floating list
//! inside the overlay's panel, named by the `…` row that opened it.
//! Split from `help_launcher.rs` to keep the painter files small.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::help::HelpRender;
use super::palette::Palette;
use super::popup_frame::{self, PopupChrome, overlay_bg};
use super::render::put_text;
use super::text::{display_width, fit_hints, truncate_end};
use crate::help_launcher::{HelpPicker, PickerRow};

fn format_picker_line(prefix: &str, row: &PickerRow, max_width: usize) -> String {
    let base = format!("{prefix}{}", row.label);
    let Some(reason) = row.dim.as_deref() else {
        return truncate_end(&base, max_width);
    };
    let tail = format!(" · {reason}");
    let tail_w = display_width(&tail);
    let prefix_w = display_width(prefix);
    if max_width > prefix_w + tail_w + 4 {
        let head = truncate_end(&row.label, max_width - prefix_w - tail_w);
        format!("{prefix}{head}{tail}")
    } else {
        truncate_end(&format!("{base}{tail}"), max_width)
    }
}

/// Visible window start offset for `picker` given `vis` displayed rows.
pub(crate) fn visible_start(picker: &HelpPicker, vis: usize) -> usize {
    if picker.rows.len() <= vis || vis == 0 {
        0
    } else if picker.selected < picker.scroll {
        picker.selected
    } else if picker.selected >= picker.scroll + vis {
        picker.selected + 1 - vis
    } else {
        picker.scroll
    }
    .min(picker.rows.len().saturating_sub(vis))
}

/// The compact target picker — a floating list inside the overlay's
/// panel, named by the row that opened it. Returns the frame and
/// per-row rects through the `HelpRender` the caller already owns.
#[cfg(test)]
pub(crate) fn render_picker(
    b: &mut Buffer,
    inside: Rect,
    picker: &HelpPicker,
    hits: &mut HelpRender,
    p: &Palette,
) {
    render_picker_with_shadow(b, inside, picker, hits, p, true);
}

pub(crate) fn render_picker_with_shadow(
    b: &mut Buffer,
    inside: Rect,
    picker: &HelpPicker,
    hits: &mut HelpRender,
    p: &Palette,
    shadow: bool,
) {
    let width = inside.width.min(48);
    let height = (picker.rows.len() as u16 + 4).min(inside.height);
    if width < 12 || height < 5 {
        return;
    }
    let Some(geom) = popup_frame::open_centered(
        b,
        inside,
        width.saturating_sub(2),
        height.saturating_sub(2),
        12,
        5,
        p.accent,
        PopupChrome::Standard,
        p,
        shadow,
    ) else {
        return;
    };
    let q = geom.outer;
    let i = geom.inner;
    let panel_bg = overlay_bg(p);
    hits.picker = q;
    put_text(
        b,
        i.x,
        i.y,
        i.width,
        &truncate_end(&format!(" {}", picker.title), i.width as usize),
        Style::default()
            .fg(p.accent)
            .bg(panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    let rows_h = i.height.saturating_sub(2) as usize;
    let start = visible_start(picker, rows_h);
    for (slot, (pos, row)) in picker
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .take(rows_h)
        .enumerate()
    {
        let y = i.y + 1 + slot as u16;
        let rect = Rect::new(i.x, y, i.width, 1);
        let selected = pos == picker.selected;
        let bg = if selected { p.selection_bg } else { panel_bg };
        let fg = if row.dim.is_some() {
            p.overlay0
        } else {
            p.text
        };
        b.set_style(rect, Style::default().bg(bg));
        let prefix = if selected { "▸ " } else { "  " };
        let text = format_picker_line(prefix, row, i.width as usize);
        put_text(b, i.x, y, i.width, &text, Style::default().fg(fg).bg(bg));
        hits.picker_rows.push((rect, pos));
    }
    let footer = if picker.standalone {
        fit_hints(
            &[
                " ↑↓ pick   ↵ reopen   esc close",
                " ↵ reopen   esc close",
                " esc close",
            ],
            usize::from(i.width),
        )
    } else {
        fit_hints(
            &[
                " ↑↓ pick   ↵ run   esc back",
                " ↵ run   esc back",
                " esc back",
            ],
            usize::from(i.width),
        )
    };
    put_text(
        b,
        i.x,
        i.bottom() - 1,
        i.width,
        footer,
        Style::default().fg(p.overlay0).bg(panel_bg),
    );
}
