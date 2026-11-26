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
use super::text::truncate_end;
use crate::help_launcher::HelpPicker;

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
    let rows_h = i.height.saturating_sub(2);
    for (pos, row) in picker.rows.iter().enumerate().take(rows_h as usize) {
        let y = i.y + 1 + pos as u16;
        let rect = Rect::new(i.x, y, i.width, 1);
        let selected = pos == picker.selected;
        let bg = if selected { p.selection_bg } else { panel_bg };
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
        Style::default().fg(p.overlay0).bg(panel_bg),
    );
}
