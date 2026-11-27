//! The per-kind dialog renderers — reason line, confirm card,
//! Tell and Revise editors (split from the shared geometry).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::super::palette::Palette;
use super::super::popup_frame::overlay_bg;
use super::super::render::{display_width, put_text};
use super::super::text::truncate_end;
use super::super::text_editor;
use super::{
    ChromeMode, DialogHits, cancel_button, centered_titled, dialog_width, dim_line, send_button,
};
use crate::studio_dialog::{BodyEditor, DialogField};

/// The reason dialog (§2.1) — one `reason:` line, Enter submits.
pub(super) fn render_reason(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ReasonDialog,
    palette: &Palette,
    mode: ChromeMode,
) -> Option<DialogHits> {
    let h = if mode.deco { 7 } else { 8 };
    let (q, i) = centered_titled(
        b,
        band,
        dialog_width(band.width),
        h,
        &d.title,
        mode,
        palette,
    )?;
    let bg = overlay_bg(palette);
    let raised = palette.elevation().raised_on_overlay;
    let row_off = if mode.deco { 1 } else { 2 };
    let input = Rect::new(i.x, i.y + row_off, i.width.saturating_sub(1), 1);
    put_text(
        b,
        i.x,
        i.y + row_off,
        9,
        " reason: ",
        Style::default().fg(palette.subtext0).bg(bg),
    );
    let field = Rect::new(i.x + 9, i.y + row_off, i.width.saturating_sub(10), 1);
    b.set_style(field, Style::default().fg(palette.text).bg(raised));
    let cursor = text_editor::render(
        b,
        field,
        &d.reason,
        Style::default().fg(palette.text).bg(raised),
    );
    let _ = input;
    if let Some(error) = &d.error {
        dim_line(
            b,
            i.x,
            i.y + row_off + 1,
            i.width,
            &format!(" ! {error}"),
            palette,
        );
    }
    let rs = super::super::dialogs::row(i, &[12, 12], 2, 4);
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(b, *ok, " ↵ submit ", true, mode, palette);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: field,
        summary: Rect::default(),
        cursor,
        ..DialogHits::default()
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
    mode: ChromeMode,
) -> Option<DialogHits> {
    let (q, i) = centered_titled(
        b,
        band,
        dialog_width(band.width),
        9,
        &d.title,
        mode,
        palette,
    )?;
    let bg = overlay_bg(palette);
    let raised = palette.elevation().raised_on_overlay;
    let body = Rect::new(i.x, i.y + 1, i.width, 4);
    let mut cursor = render_body(
        b,
        body,
        &d.body,
        Style::default().fg(palette.text).bg(raised),
        palette,
    );
    if let Some(error) = &d.error {
        put_text(
            b,
            i.x,
            i.y + 5,
            i.width,
            &truncate_end(&format!(" ! {error}"), i.width as usize),
            Style::default().fg(palette.red).bg(bg),
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
    let rs = super::super::dialogs::row(i, &[12, 12], 2, i.height.saturating_sub(1));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(
        b,
        *ok,
        " ^s send ",
        d.field == DialogField::Send,
        mode,
        palette,
    );
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
        ..DialogHits::default()
    })
}

/// The Revise dialog (§2.4) — prefilled body rows, the required
/// one-line `Summary of changes`, `^e` a local `$EDITOR`.
pub(super) fn render_revise(
    b: &mut Buffer,
    band: Rect,
    d: &crate::studio_dialog::ReviseDialog,
    palette: &Palette,
    mode: ChromeMode,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let body_rows = 10u16.min(band.height.saturating_sub(9)).max(3);
    let height = body_rows + 8;
    let (q, i) = centered_titled(
        b,
        band,
        width,
        height.min(band.height.saturating_sub(2)).max(9),
        &d.title,
        mode,
        palette,
    )?;
    let bg = overlay_bg(palette);
    let raised = palette.elevation().raised_on_overlay;
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
                Style::default().fg(palette.text).bg(raised),
                palette,
            );
        }
        None => {
            b.set_style(body, Style::default().bg(raised));
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
        Style::default().fg(palette.subtext0).bg(bg),
    );
    let summary = Rect::new(
        i.x + 20.min(i.width.saturating_sub(1)),
        summary_y,
        i.width.saturating_sub(20),
        1,
    );
    b.set_style(summary, Style::default().fg(palette.text).bg(raised));
    if let Some(c) = text_editor::render(
        b,
        summary,
        &d.summary,
        Style::default().fg(palette.text).bg(raised),
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
            Style::default().fg(palette.red).bg(bg),
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
    let rs = super::super::dialogs::row(i, &[14, 12], 2, i.height.saturating_sub(1));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(
        b,
        *ok,
        " ^s submit ",
        d.field == DialogField::Send,
        mode,
        palette,
    );
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
        ..DialogHits::default()
    })
}
