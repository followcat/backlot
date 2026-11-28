//! Permission list dialog chrome: grant rows, scope/rule details and
//! the revoke hint, with rows click-selectable like the other dialogs.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::super::palette::Palette;
use super::super::popup_frame::overlay_bg;
use super::super::render::put_text;
use super::super::text::truncate_end;
use super::{ChromeMode, DialogHits, cancel_button, centered_titled, dialog_width};
use crate::hollywood::wrap_plain;
use crate::studio_dialog::PermissionsDialog;

pub(super) fn render_permissions(
    b: &mut Buffer,
    band: Rect,
    d: &PermissionsDialog,
    palette: &Palette,
    mode: ChromeMode,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let height = 18.min(band.height.saturating_sub(2).max(9));
    let (q, i) = centered_titled(b, band, width, height, &d.title(), mode, palette)?;
    let bg = overlay_bg(palette);
    let dim = Style::default().fg(palette.subtext0).bg(bg);
    if d.permits.is_empty() {
        put_text(
            b,
            i.x,
            i.y + 2,
            i.width,
            "  No remembered permissions.",
            dim,
        );
    }
    let list_h = i.height.saturating_sub(5) as usize;
    let mut rects = Vec::new();
    for (idx, permit) in d.permits.iter().take(list_h).enumerate() {
        let y = i.y + 1 + idx as u16;
        let selected = idx == d.selected;
        let rect = Rect::new(i.x, y, i.width, 1);
        rects.push(rect);
        let style = if selected {
            Style::default()
                .fg(palette.text)
                .bg(palette.elevation().raised_on_overlay)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text).bg(bg)
        };
        if selected {
            b.set_style(rect, style);
        }
        let scope = match permit.scope {
            backlot_model::event::PermitScope::Studio => "studio",
            backlot_model::event::PermitScope::Role => "role",
        };
        let mark = if selected { " ▸ " } else { "   " };
        put_text(
            b,
            i.x,
            y,
            i.width,
            &truncate_end(
                &format!(
                    "{mark}{} · {:?} · {}",
                    scope, permit.permit_kind, permit.rule
                ),
                i.width as usize,
            ),
            style,
        );
    }
    let hint_y = i.bottom().saturating_sub(3);
    let hint = if d.permits.is_empty() {
        "esc close".to_string()
    } else {
        "enter/r revoke · esc close".to_string()
    };
    for line in wrap_plain(&hint, i.width.saturating_sub(2) as usize, 0)
        .iter()
        .take(1)
    {
        put_text(b, i.x, hint_y, i.width, &format!(" {line}"), dim);
    }
    let mut cancel_rect = Rect::default();
    let rs = super::super::dialogs::row(i, &[13], 2, i.height.saturating_sub(1));
    if let [cancel] = rs.as_slice() {
        cancel_button(b, *cancel, palette);
        cancel_rect = *cancel;
    }
    Some(DialogHits {
        area: q,
        cancel: cancel_rect,
        choices: rects,
        ..DialogHits::default()
    })
}
