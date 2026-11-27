//! The S3 native dialogs' chrome (§2): the reason line, the confirm
//! card, the 4-row Tell editor and the prefilled Revise editor —
//! every kind centred in the `modal_band()` rect the app hands over,
//! so a beside dock's border is never crossed (A6.2).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::dialogs::{button, contrast};
use super::palette::Palette;
use super::popup_frame::{self, PopupChrome, overlay_bg};
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
    /// Clickable option rows for choice dialogs (`DesignSign`).
    pub choices: Vec<Rect>,
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

/// Open a standard padded/guttered dialog frame centered in `band`.
pub(super) fn centered_panel(
    b: &mut Buffer,
    band: Rect,
    w: u16,
    h: u16,
    palette: &Palette,
    shadow: bool,
) -> Option<(Rect, Rect)> {
    let w = w.min(band.width.saturating_sub(4));
    let h = h.min(band.height.saturating_sub(2));
    if w < 10 || h < 5 {
        return None;
    }
    let geom = popup_frame::open_centered(
        b,
        band,
        w.saturating_sub(2),
        h.saturating_sub(2),
        10,
        5,
        palette.accent,
        PopupChrome::Standard,
        palette,
        shadow,
    )?;
    Some((geom.outer, geom.inner))
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
            .bg(overlay_bg(palette))
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
        Style::default()
            .fg(palette.overlay0)
            .bg(overlay_bg(palette)),
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
            .bg(palette.elevation().raised_on_overlay)
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
            .bg(palette.elevation().raised_on_overlay)
            .add_modifier(Modifier::BOLD),
    );
}

mod archived;
mod design_sign;
mod kinds;
mod lifecycle;
use archived::render_restore;
use design_sign::render_design_sign;
use kinds::{render_confirm, render_reason, render_revise, render_tell};
use lifecycle::{render_cast_edit, render_recast, render_restart};

/// One render entry — the app hands the `modal_band()` rect so a
/// beside dock's border is never crossed (A6.2).
#[cfg(test)]
pub(crate) fn render(
    b: &mut Buffer,
    band: Rect,
    dialog: &StudioDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    render_with_shadow(b, band, dialog, palette, true)
}

pub(crate) fn render_with_shadow(
    b: &mut Buffer,
    band: Rect,
    dialog: &StudioDialog,
    palette: &Palette,
    shadow: bool,
) -> Option<DialogHits> {
    match dialog {
        StudioDialog::Reason(d) => render_reason(b, band, d, palette, shadow),
        StudioDialog::Confirm(d) => render_confirm(b, band, d, palette, shadow),
        StudioDialog::DesignSign(d) => render_design_sign(b, band, d, palette, shadow),
        StudioDialog::Tell(d) => render_tell(b, band, d, palette, shadow),
        StudioDialog::Revise(d) => render_revise(b, band, d, palette, shadow),
        StudioDialog::Restart(d) => render_restart(b, band, d, palette, shadow),
        StudioDialog::Recast(d) => render_recast(b, band, d, palette, shadow),
        StudioDialog::Restore(d) => render_restore(b, band, d, palette, shadow),
        StudioDialog::CastEdit(d) => render_cast_edit(b, band, d, palette, shadow),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
