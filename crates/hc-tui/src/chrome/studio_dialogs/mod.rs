//! The S3 native dialogs' chrome (§2): the reason line, the confirm
//! card, the 4-row Tell editor and the prefilled Revise editor —
//! every kind centred in the `modal_band()` rect the app hands over,
//! so a beside dock's border is never crossed (A6.2).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::dialogs::{button, contrast};
use super::palette::Palette;
use super::render::put_text;
use super::text::truncate_end;
use crate::studio_dialog::StudioDialog;

/// The rectangles a press can hit — the card itself, `Send`/`OK`,
/// `Cancel` and the body editor's rows.
#[derive(Default)]
pub(crate) struct DialogHits {
    pub area: Rect,
    pub send: Rect,
    pub cancel: Rect,
    /// The Tell/Revise body rows — a click inside focuses the editor.
    pub body: Rect,
    /// The revise summary line.
    pub summary: Rect,
    pub cursor: Option<super::help::CursorState>,
}

/// §2's responsive width: 64 cols at 120+/160, 72 under it to 80,
/// 56 to 60, 38 below — then the band's own width is the cap.
pub(crate) fn dialog_width(band_cols: u16) -> u16 {
    let spec = match band_cols {
        c if c >= 120 => 64,
        c if c >= 80 => 72,
        c if c >= 60 => 56,
        _ => 38,
    };
    spec.min(band_cols.saturating_sub(4)).max(10)
}

/// Centre `w × h` inside `band` — the caller's `modal_band()` keeps
/// the rect clear of a beside dock.
fn centered(band: Rect, w: u16, h: u16) -> Option<Rect> {
    let w = w.min(band.width.saturating_sub(4));
    let h = h.min(band.height.saturating_sub(2));
    if w < 10 || h < 5 {
        return None;
    }
    Some(Rect::new(
        band.x + (band.width - w) / 2,
        band.y + (band.height - h) / 2,
        w,
        h,
    ))
}

fn title_line(b: &mut Buffer, i: Rect, title: &str, palette: &Palette) {
    put_text(
        b,
        i.x,
        i.y,
        i.width,
        &truncate_end(&format!(" {title}"), i.width as usize),
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
}

fn dim_line(b: &mut Buffer, x: u16, y: u16, w: u16, text: &str, palette: &Palette) {
    put_text(
        b,
        x,
        y,
        w,
        &truncate_end(text, w as usize),
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
}

fn send_button(b: &mut Buffer, row: Rect, label: &str, focused: bool, palette: &Palette) {
    let style = if focused {
        Style::default()
            .fg(contrast(palette))
            .bg(palette.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.accent)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD)
    };
    button(b, row, label, style);
}

fn cancel_button(b: &mut Buffer, row: Rect, palette: &Palette) {
    button(
        b,
        row,
        " esc cancel ",
        Style::default()
            .fg(palette.text)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
}

mod archived;
mod kinds;
mod lifecycle;
use archived::render_restore;
use kinds::{render_confirm, render_reason, render_revise, render_tell};
use lifecycle::{render_cast_edit, render_recast, render_restart};

/// One render entry — the app hands the `modal_band()` rect so a
/// beside dock's border is never crossed (A6.2).
pub(crate) fn render(
    b: &mut Buffer,
    band: Rect,
    dialog: &StudioDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    match dialog {
        StudioDialog::Reason(d) => render_reason(b, band, d, palette),
        StudioDialog::Confirm(d) => render_confirm(b, band, d, palette),
        StudioDialog::Tell(d) => render_tell(b, band, d, palette),
        StudioDialog::Revise(d) => render_revise(b, band, d, palette),
        StudioDialog::Restart(d) => render_restart(b, band, d, palette),
        StudioDialog::Recast(d) => render_recast(b, band, d, palette),
        StudioDialog::Restore(d) => render_restore(b, band, d, palette),
        StudioDialog::CastEdit(d) => render_cast_edit(b, band, d, palette),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
