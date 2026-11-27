//! Chrome renderer for the opening-design sign-off choice dialog
//! (`StudioDialog::DesignSign`): wraps every proposed check's name, full `run`
//! command, timeout, and optional `expect` regex cleanly without truncation,
//! followed by the three selectable choices and confirm/cancel buttons.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::super::palette::Palette;
use super::super::popup_frame::overlay_bg;
use super::super::render::put_text;
use super::super::text::truncate_end;
use super::{DialogHits, cancel_button, centered_panel, dialog_width, send_button};
use crate::hollywood::wrap_plain;
use crate::studio_dialog::{DesignSignChoice, DesignSignDialog};

struct CheckLine {
    text: String,
    accent: bool,
    bold: bool,
}

fn build_check_lines(d: &DesignSignDialog, inner_w: usize) -> Vec<CheckLine> {
    let wrap_w = inner_w.saturating_sub(2).max(8);
    let mut lines = Vec::new();
    for row in wrap_plain(&d.summary_line, wrap_w, 0) {
        lines.push(CheckLine {
            text: row,
            accent: false,
            bold: false,
        });
    }
    for c in &d.checks {
        lines.push(CheckLine {
            text: String::new(),
            accent: false,
            bold: false,
        });
        for row in wrap_plain(&format!("{} (timeout {}s)", c.name, c.timeout_s), wrap_w, 0) {
            lines.push(CheckLine {
                text: row,
                accent: false,
                bold: true,
            });
        }
        for row in wrap_plain(&format!("  run: {}", c.run), wrap_w, 0) {
            lines.push(CheckLine {
                text: row,
                accent: false,
                bold: false,
            });
        }
        if let Some(pat) = &c.expect {
            let min = c.effective_min_matches();
            let exp = if min == 1 {
                format!("  expect: {pat}")
            } else {
                format!("  expect: {pat} (min {min})")
            };
            for row in wrap_plain(&exp, wrap_w, 0) {
                lines.push(CheckLine {
                    text: row,
                    accent: true,
                    bold: false,
                });
            }
        }
    }
    lines
}

pub(super) fn render_design_sign(
    b: &mut Buffer,
    band: Rect,
    d: &DesignSignDialog,
    palette: &Palette,
    shadow: bool,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let inner_w = width.saturating_sub(4).max(10) as usize;
    let body_lines = build_check_lines(d, inner_w);
    // Chrome layout inside `i`:
    //   1 title row + N check rows + 1 gap row + 3 choice rows + 1 gap + 1 button row + 2 frame chrome rows
    let ideal_h = (body_lines.len() as u16).saturating_add(9);
    let max_h = band.height.saturating_sub(2).max(8);
    let height = ideal_h.min(max_h);
    let (q, i) = centered_panel(b, band, width, height, palette, shadow)?;
    let bg = overlay_bg(palette);
    let raised = palette.elevation().raised_on_overlay;

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

    // Reserve bottom rows: 3 choices + 1 blank/button gap + 1 button row (when height allows).
    let fixed_bottom = if i.height >= 7 { 5 } else { 3 };
    let body_top = i.y + 1;
    let body_h = i.height.saturating_sub(1 + fixed_bottom) as usize;
    let max_scroll = body_lines.len().saturating_sub(body_h);
    let start = d.scroll.min(max_scroll);
    for (idx, line) in body_lines.iter().skip(start).take(body_h).enumerate() {
        let y = body_top + idx as u16;
        if line.text.is_empty() {
            continue;
        }
        let mut style = Style::default()
            .fg(if line.accent {
                palette.teal
            } else {
                palette.text
            })
            .bg(bg);
        if line.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        put_text(b, i.x, y, i.width, &format!(" {}", line.text), style);
    }

    let choices_y = if i.height >= 7 {
        i.bottom().saturating_sub(5)
    } else {
        i.bottom().saturating_sub(3)
    };
    let mut choice_rects = Vec::with_capacity(3);
    for (idx, (_kind, label)) in d.choices().into_iter().enumerate() {
        let y = choices_y + idx as u16;
        if y >= i.bottom() {
            break;
        }
        let selected = idx == d.selected;
        let row_rect = Rect::new(i.x, y, i.width, 1);
        choice_rects.push(row_rect);
        let style = if selected {
            Style::default()
                .fg(palette.text)
                .bg(raised)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.subtext0).bg(bg)
        };
        if selected {
            b.set_style(row_rect, style);
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
        let rs = super::super::dialogs::row(i, &[18, 13], 2, i.height.saturating_sub(1));
        if let [ok, cancel] = rs.as_slice() {
            let ok_label = match d.chosen() {
                DesignSignChoice::SignWithChecks => " ↵ sign + checks ",
                DesignSignChoice::SignWithoutChecks => " ↵ no checks ",
                DesignSignChoice::Cancel => " ↵ close ",
            };
            send_button(b, *ok, ok_label, true, palette);
            cancel_button(b, *cancel, palette);
            send_rect = *ok;
            cancel_rect = *cancel;
        }
    }

    Some(DialogHits {
        area: q,
        send: send_rect,
        cancel: cancel_rect,
        choices: choice_rects,
        ..DialogHits::default()
    })
}
