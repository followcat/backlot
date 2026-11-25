//! The per-kind dialog renderers — reason line, confirm card,
//! Tell and Revise editors (split from the shared geometry).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, Widget};

use super::super::dialogs::{button, contrast, panel};
use super::super::palette::Palette;
use super::super::render::{display_width, put_text};
use super::super::text::truncate_end;
use super::super::text_editor;
use super::{DialogHits, cancel_button, centered, dialog_width, dim_line, send_button, title_line};
use crate::studio_dialog::{BodyEditor, DialogField};

/// The reason dialog (§2.1) — one `reason:` line, Enter submits.
pub(super) fn render_reason(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ReasonDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let q = centered(band, dialog_width(band.width), 8)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    let input = Rect::new(i.x, i.y + 2, i.width.saturating_sub(1), 1);
    put_text(
        b,
        i.x,
        i.y + 2,
        9,
        " reason: ",
        Style::default().fg(palette.subtext0).bg(palette.panel_bg),
    );
    let field = Rect::new(i.x + 9, i.y + 2, i.width.saturating_sub(10), 1);
    b.set_style(
        field,
        Style::default().fg(palette.text).bg(palette.surface0),
    );
    let cursor = text_editor::render(
        b,
        field,
        &d.reason,
        Style::default().fg(palette.text).bg(palette.surface0),
    );
    let _ = input;
    if let Some(error) = &d.error {
        dim_line(b, i.x, i.y + 3, i.width, &format!(" ! {error}"), palette);
    }
    let rs = super::super::dialogs::row(i, &[12, 12], 2, 4);
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(b, *ok, " ↵ submit ", true, palette);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: field,
        summary: Rect::default(),
        cursor,
    })
}

/// The confirm dialog (§2.2) — the wrapped explanation, `↵/y` and
/// `esc`.
pub(super) fn render_confirm(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ConfirmDialog,
    palette: &Palette,
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
        let mut line = String::new();
        for word in para.split_whitespace() {
            let word_w = usize::from(display_width(word));
            let line_w = usize::from(display_width(&line));
            if line.is_empty() {
                if word_w <= inner {
                    line.push_str(word);
                } else {
                    lines.push(truncate_end(word, inner));
                }
            } else if line_w + 1 + word_w <= inner {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(line);
                line = if word_w <= inner {
                    word.to_owned()
                } else {
                    lines.push(truncate_end(word, inner));
                    String::new()
                };
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    let height = (lines.len() as u16)
        .saturating_add(6)
        .min(band.height.saturating_sub(2));
    let q = centered(band, width, height)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    put_text(
        b,
        i.x,
        i.y,
        i.width,
        &truncate_end(&format!(" {}", d.title), i.width as usize),
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
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
            Style::default().fg(palette.text).bg(palette.panel_bg),
        );
    }
    let rs = super::super::dialogs::row(i, &[19, 13], 2, i.height.saturating_sub(2));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    let ok_style = if d.loading {
        Style::default().fg(palette.overlay0).bg(palette.surface0)
    } else {
        Style::default()
            .fg(contrast(palette))
            .bg(palette.accent)
            .add_modifier(Modifier::BOLD)
    };
    button(b, *ok, " ↵ / y confirm ", ok_style);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor: None,
    })
}

/// The multi-line body rows — each line drawn, the cursor's row
/// kept in view, the `CursorState` handed back.
fn render_body(
    b: &mut Buffer,
    area: Rect,
    editor: &BodyEditor,
    style: Style,
    palette: &Palette,
) -> Option<super::super::help::CursorState> {
    if area.is_empty() {
        return None;
    }
    b.set_style(area, style);
    // Keep the cursor row inside the window.
    let scroll = editor
        .row
        .saturating_sub(area.height.saturating_sub(1) as usize);
    let mut cursor = None;
    for line in 0..area.height {
        let Some(row) = editor.lines.get(scroll + line as usize) else {
            break;
        };
        let text: String = row.iter().collect();
        put_text(b, area.x, area.y + line, area.width, &text, style);
        if scroll + line as usize == editor.row {
            let prefix: String = row[..editor.col.min(row.len())].iter().collect();
            let cx = area.x + display_width(&prefix).min(area.width);
            cursor = Some(super::super::help::CursorState {
                x: cx,
                y: area.y + line,
                visible: true,
            });
        }
    }
    let _ = palette;
    cursor
}

/// The Tell dialog (§2.3) — a 4-row editor, the queued-delivery
/// note, `^s`/`Tab → Send` (and `Enter` on a single line) submits.
pub(super) fn render_tell(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::TellDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let q = centered(band, dialog_width(band.width), 9)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    let body = Rect::new(i.x, i.y + 1, i.width, 4);
    let mut cursor = render_body(
        b,
        body,
        &d.body,
        Style::default().fg(palette.text).bg(palette.surface0),
        palette,
    );
    if let Some(error) = &d.error {
        put_text(
            b,
            i.x,
            i.y + 5,
            i.width,
            &truncate_end(&format!(" ! {error}"), i.width as usize),
            Style::default().fg(palette.red).bg(palette.panel_bg),
        );
    } else {
        dim_line(
            b,
            i.x,
            i.y + 5,
            i.width,
            " Queued in ledger; stage delivers when the role is idle.",
            palette,
        );
    }
    let rs = super::super::dialogs::row(i, &[10, 12], 2, i.height.saturating_sub(1));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(b, *ok, " ^s send ", d.field == DialogField::Send, palette);
    cancel_button(b, *cancel, palette);
    if d.field == DialogField::Send {
        cursor = None;
    }
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body,
        summary: Rect::default(),
        cursor,
    })
}

/// The Revise dialog (§2.4) — prefilled body rows, the required
/// one-line `Summary of changes`, `^e` a local `$EDITOR`.
pub(super) fn render_revise(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ReviseDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let body_rows = 10u16.min(band.height.saturating_sub(9)).max(3);
    let height = body_rows + 8;
    let q = centered(
        band,
        width,
        height.min(band.height.saturating_sub(2)).max(9),
    )?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    let body = Rect::new(
        i.x,
        i.y + 1,
        i.width,
        body_rows.min(i.height.saturating_sub(5)),
    );
    let mut cursor = None;
    match &d.body {
        Some(editor) => {
            cursor = render_body(
                b,
                body,
                editor,
                Style::default().fg(palette.text).bg(palette.surface0),
                palette,
            );
        }
        None => {
            b.set_style(body, Style::default().bg(palette.surface0));
            dim_line(
                b,
                body.x,
                body.y,
                body.width,
                " loading requirement…",
                palette,
            );
        }
    }
    let summary_y = body.bottom();
    put_text(
        b,
        i.x,
        summary_y,
        19.min(i.width),
        " Summary of changes: ",
        Style::default().fg(palette.subtext0).bg(palette.panel_bg),
    );
    let summary = Rect::new(
        i.x + 20.min(i.width.saturating_sub(1)),
        summary_y,
        i.width.saturating_sub(20),
        1,
    );
    b.set_style(
        summary,
        Style::default().fg(palette.text).bg(palette.surface0),
    );
    if let Some(c) = text_editor::render(
        b,
        summary,
        &d.summary,
        Style::default().fg(palette.text).bg(palette.surface0),
    ) && d.field == DialogField::Summary
    {
        cursor = Some(c);
    }
    let hint_y = summary_y + 1;
    match &d.error {
        Some(error) => put_text(
            b,
            i.x,
            hint_y,
            i.width,
            &truncate_end(&format!(" ! {error}"), i.width as usize),
            Style::default().fg(palette.red).bg(palette.panel_bg),
        ),
        None => {
            let hint = if d.local {
                crate::chrome::text::fit_hints(
                    &[
                        " ^e open $EDITOR  tab field  ^s submit  esc cancel",
                        " ^e  tab  ^s submit  esc",
                        " ^s  esc",
                    ],
                    usize::from(i.width),
                )
            } else {
                crate::chrome::text::fit_hints(
                    &[" tab field  ^s submit  esc cancel", " ^s submit  esc"],
                    usize::from(i.width),
                )
            };
            dim_line(b, i.x, hint_y, i.width, hint, palette);
        }
    }
    let rs = super::super::dialogs::row(i, &[12, 12], 2, i.height.saturating_sub(1));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(b, *ok, " ^s submit ", d.field == DialogField::Send, palette);
    cancel_button(b, *cancel, palette);
    match d.field {
        DialogField::Body => {}
        _ => {
            if d.field == DialogField::Send {
                cursor = None;
            }
        }
    }
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body,
        summary,
        cursor,
    })
}
