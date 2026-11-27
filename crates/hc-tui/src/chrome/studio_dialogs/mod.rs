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

#[derive(Clone, Copy)]
pub(super) struct ChromeMode {
    pub(super) deco: bool,
    pub(super) shadow: bool,
    pub(super) destructive: bool,
}

impl ChromeMode {
    pub(super) fn border_fg(self, palette: &Palette) -> ratatui::style::Color {
        if self.destructive {
            palette.red
        } else {
            palette.accent
        }
    }
}

/// Open a dialog frame centered in `band`. When `mode.deco` is true, renders a
/// `PopupChrome::Deco` frame with `title` in the top border plaque and returns
/// an interior `Rect` aligned to the caller's post-title body offsets.
pub(super) fn centered_titled(
    b: &mut Buffer,
    band: Rect,
    w: u16,
    h: u16,
    title: &str,
    mode: ChromeMode,
    palette: &Palette,
) -> Option<(Rect, Rect)> {
    let w = w.min(band.width.saturating_sub(4));
    let h = h.min(band.height.saturating_sub(2));
    if w < 10 || h < 5 {
        return None;
    }
    if mode.deco {
        let content_h = h.saturating_sub(3).max(4);
        let border_fg = mode.border_fg(palette);
        let geom = popup_frame::open_centered(
            b,
            band,
            w.saturating_sub(2),
            content_h,
            10,
            4,
            border_fg,
            PopupChrome::Deco,
            palette,
            mode.shadow,
        )?;
        super::popup_deco::draw_deco_title(
            b,
            geom.border,
            title,
            border_fg,
            mode.destructive,
            palette,
        );
        let virtual_inner = Rect::new(
            geom.inner.x,
            geom.inner.y.saturating_sub(1),
            geom.inner.width,
            geom.inner.height.saturating_add(1),
        );
        Some((geom.outer, virtual_inner))
    } else {
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
            mode.shadow,
        )?;
        title_line(b, geom.inner, title, palette);
        Some((geom.outer, geom.inner))
    }
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

fn send_button(
    b: &mut Buffer,
    row: Rect,
    label: &str,
    focused: bool,
    mode: ChromeMode,
    palette: &Palette,
) {
    let accent = mode.border_fg(palette);
    let style = if focused {
        Style::default()
            .fg(contrast(palette))
            .bg(accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(accent)
            .bg(palette.elevation().raised_on_overlay)
            .add_modifier(Modifier::BOLD)
    };
    let text = super::popup_deco::primary_label(label, mode.deco);
    button(b, row, &text, style);
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
mod confirm;
mod design_sign;
mod kinds;
mod lifecycle;
use archived::render_restore;
use confirm::render_confirm;
use design_sign::render_design_sign;
use kinds::{render_reason, render_revise, render_tell};
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

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_with_shadow(
    b: &mut Buffer,
    band: Rect,
    dialog: &StudioDialog,
    palette: &Palette,
    shadow: bool,
) -> Option<DialogHits> {
    render_styled(b, band, dialog, palette, shadow, true)
}

pub(crate) fn render_styled(
    b: &mut Buffer,
    band: Rect,
    dialog: &StudioDialog,
    palette: &Palette,
    shadow: bool,
    deco: bool,
) -> Option<DialogHits> {
    let mode = ChromeMode {
        deco,
        shadow,
        destructive: is_destructive(dialog),
    };
    match dialog {
        StudioDialog::Reason(d) => render_reason(b, band, d, palette, mode),
        StudioDialog::Confirm(d) => render_confirm(b, band, d, palette, mode),
        StudioDialog::DesignSign(d) => render_design_sign(b, band, d, palette, mode),
        StudioDialog::Tell(d) => render_tell(b, band, d, palette, mode),
        StudioDialog::Revise(d) => render_revise(b, band, d, palette, mode),
        StudioDialog::Restart(d) => render_restart(b, band, d, palette, mode),
        StudioDialog::Recast(d) => render_recast(b, band, d, palette, mode),
        StudioDialog::Restore(d) => render_restore(b, band, d, palette, mode),
        StudioDialog::CastEdit(d) => render_cast_edit(b, band, d, palette, mode),
    }
}

fn is_destructive(dialog: &StudioDialog) -> bool {
    use crate::studio_dialog::{ConfirmFor, ReasonFor};
    match dialog {
        StudioDialog::Confirm(d) => matches!(
            d.for_,
            ConfirmFor::DeleteStudio { .. }
                | ConfirmFor::StageStop { .. }
                | ConfirmFor::Close { .. }
                | ConfirmFor::Prune { .. }
        ),
        StudioDialog::Reason(d) => match &d.for_ {
            ReasonFor::TaskVerb { verb, .. } => *verb == "abandon",
            ReasonFor::CardCancel { .. } => true,
            ReasonFor::Finish { .. } => false,
        },
        _ => false,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
