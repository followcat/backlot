//! Permission approval choice dialog chrome: the command/detail, exact
//! remember rules, outward/once-only explanation, and Babylon choice rows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::super::palette::Palette;
use super::super::popup_frame::overlay_bg;
use super::super::render::put_text;
use super::super::text::truncate_end;
use super::{ChromeMode, DialogHits, cancel_button, centered_titled, dialog_width, send_button};
use crate::hollywood::wrap_plain;
use crate::studio_dialog::{ApprovalChoice, ApprovalDialog};

pub(super) fn render_approval(
    b: &mut Buffer,
    band: Rect,
    d: &ApprovalDialog,
    palette: &Palette,
    mode: ChromeMode,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let inner_w = width.saturating_sub(4).max(10) as usize;
    let wrap_w = inner_w.saturating_sub(2).max(8);
    let mut body = Vec::new();
    body.extend(wrap_plain(&format!("prompt: {}", d.detail), wrap_w, 0));
    if let Some(why) = &d.once_only {
        body.push(String::new());
        body.extend(wrap_plain(&format!("once only: {why}"), wrap_w, 0));
    } else if d.outward {
        body.push(String::new());
        body.extend(wrap_plain(
            "outward action: studio-wide remembering is unavailable",
            wrap_w,
            0,
        ));
    }
    let choices = d.choices();
    let ideal_h = (body.len() as u16).saturating_add(choices.len() as u16 + 7);
    let height = ideal_h.min(band.height.saturating_sub(2).max(9));
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
    let fixed_bottom = choices.len() as u16 + 3;
    let body_h = i.height.saturating_sub(fixed_bottom + 1) as usize;
    for (idx, line) in body.iter().take(body_h).enumerate() {
        let dim = line.starts_with("once only:") || line.starts_with("outward action:");
        put_text(
            b,
            i.x,
            i.y + 1 + idx as u16,
            i.width,
            &format!(" {line}"),
            Style::default()
                .fg(if dim { palette.subtext0 } else { palette.text })
                .bg(bg),
        );
    }
    let choices_y = i.bottom().saturating_sub(choices.len() as u16 + 2);
    let mut rects = Vec::new();
    for (idx, (_choice, label)) in choices.iter().enumerate() {
        let y = choices_y + idx as u16;
        if y >= i.bottom().saturating_sub(1) {
            break;
        }
        let selected = idx == d.selected;
        let rect = Rect::new(i.x, y, i.width, 1);
        rects.push(rect);
        let style = if selected {
            Style::default()
                .fg(palette.text)
                .bg(raised)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.subtext0).bg(bg)
        };
        if selected {
            b.set_style(rect, style);
        }
        let mark = if selected { " ▸ " } else { "   " };
        put_text(
            b,
            i.x,
            y,
            i.width,
            &truncate_end(&format!("{mark}{label}"), i.width as usize),
            style,
        );
    }
    let mut send_rect = Rect::default();
    let mut cancel_rect = Rect::default();
    if i.height >= 7 {
        let rs = super::super::dialogs::row(i, &[16, 13], 2, i.height.saturating_sub(1));
        if let [ok, cancel] = rs.as_slice() {
            let label = match d.chosen() {
                ApprovalChoice::Deny => " ↵ deny ",
                _ => " ↵ allow ",
            };
            send_button(b, *ok, label, true, mode, palette);
            cancel_button(b, *cancel, palette);
            send_rect = *ok;
            cancel_rect = *cancel;
        }
    }
    Some(DialogHits {
        area: q,
        send: send_rect,
        cancel: cancel_rect,
        choices: rects,
        ..DialogHits::default()
    })
}
