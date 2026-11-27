//! The studio confirm dialog (`DeleteStudio`, `StageStop`, `Close`,
//! `Prune`, `FinishEmpty`, `ClearAttention`, `DirOverride`, simple `DesignSign`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::super::dialogs::{button, contrast};
use super::super::palette::Palette;
use super::super::popup_deco::primary_label;
use super::super::popup_frame::overlay_bg;
use super::super::render::put_text;
use super::super::text::truncate_end;
use super::{ChromeMode, DialogHits, cancel_button, centered_titled, dialog_width};

/// The confirm dialog (§2.2) — the wrapped explanation, `↵/y` and
/// `esc`.
pub(super) fn render_confirm(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ConfirmDialog,
    palette: &Palette,
    mode: ChromeMode,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    // Wrap the body at the inner width so nothing clips (A8.x rule:
    // no silent clipping).
    let inner = width.saturating_sub(4).max(8) as usize;
    // A refused confirm (a stale server plan) is part of the card, not
    // a toast: the line and the re-review offer read where the person
    // is looking.
    let body = match &d.error {
        Some(error) => format!("{}\n\n{error}\n\nr review again", d.body),
        None => d.body.clone(),
    };
    let mut lines = Vec::new();
    for para in body.split('\n') {
        if para.trim().is_empty() {
            continue;
        }
        lines.extend(crate::hollywood::wrap_plain(para.trim(), inner, 0));
    }
    let height = (lines.len() as u16)
        .saturating_add(6)
        .min(band.height.saturating_sub(2));
    let (q, i) = centered_titled(b, band, width, height, &d.title, mode, palette)?;
    let bg = overlay_bg(palette);
    let raised = palette.elevation().raised_on_overlay;
    if !mode.deco {
        put_text(
            b,
            i.x,
            i.y,
            i.width,
            &truncate_end(&format!(" {}", d.title), i.width as usize),
            Style::default()
                .fg(palette.accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    for (n, line) in lines.iter().enumerate() {
        let y = i.y + 1 + n as u16;
        if y >= i.bottom() - 2 {
            break;
        }
        put_text(
            b,
            i.x,
            y,
            i.width,
            &format!(" {line}"),
            Style::default().fg(palette.text).bg(bg),
        );
    }
    let rs = super::super::dialogs::row(i, &[19, 13], 2, i.height.saturating_sub(2));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    let ok_style = if d.loading {
        Style::default().fg(palette.overlay0).bg(raised)
    } else {
        Style::default()
            .fg(contrast(palette))
            .bg(mode.border_fg(palette))
            .add_modifier(Modifier::BOLD)
    };
    button(
        b,
        *ok,
        &primary_label(" ↵ / y confirm ", mode.deco),
        ok_style,
    );
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor: None,
        ..DialogHits::default()
    })
}
