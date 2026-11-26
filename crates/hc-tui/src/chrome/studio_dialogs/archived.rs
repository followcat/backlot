//! The archived-studios dialog's chrome (W4b): the list, the editable
//! restore name, and the `--dry-run` plan before the real restore.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::super::dialogs::{button, contrast, panel, row};
use super::super::palette::Palette;
use super::super::render::{display_width, put_right_text, put_text};
use super::super::text::truncate_end;
use super::super::text_editor;
use super::{DialogHits, cancel_button, centered, dialog_width, dim_line, send_button, title_line};
use crate::studio_dialog::{RestoreDialog, RestoreStep};
/// The archived list, its name field, and the dry-run plan.
pub(super) fn render_restore(
    b: &mut Buffer,
    band: Rect,
    d: &RestoreDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let height = match d.step {
        RestoreStep::List => 16,
        RestoreStep::Name => 9,
        RestoreStep::Confirm => 18,
    };
    let q = centered(band, width, height)?;
    crate::chrome::wide::clear_overlay(b, q);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title(), palette);
    let mut cursor = None;
    match d.step {
        RestoreStep::List => {
            let rows = i.height.saturating_sub(5) as usize;
            let shown = rows / 2;
            for (n, entry) in d.entries.iter().take(shown).enumerate() {
                let y = i.y + 1 + (n * 2) as u16;
                let selected = n == d.sel;
                let style = if selected {
                    Style::default()
                        .fg(palette.text)
                        .bg(palette.surface0)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.text).bg(palette.panel_bg)
                };
                if selected {
                    b.set_style(Rect::new(i.x, y, i.width, 2), style);
                }
                b.set_style(Rect::new(i.x, y + 1, i.width, 1), style);
                let mark = if selected { "▸ " } else { "  " };
                put_text(
                    b,
                    i.x,
                    y,
                    i.width,
                    &truncate_end(&format!("{mark}{}", entry.name), i.width as usize),
                    style,
                );
                put_right_text(b, i, y, &entry.stamp, style);
                let detail = [entry.repo.as_deref(), entry.requirement.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                put_text(
                    b,
                    i.x,
                    y + 1,
                    i.width,
                    &truncate_end(&format!("   {detail}"), i.width as usize),
                    Style::default().fg(palette.subtext0).bg(if selected {
                        palette.surface0
                    } else {
                        palette.panel_bg
                    }),
                );
            }
            dim_line(
                b,
                i.x,
                i.bottom().saturating_sub(3),
                i.width,
                " ↑↓ move  ⏎ restore as…",
                palette,
            );
        }
        RestoreStep::Name => {
            put_text(
                b,
                i.x,
                i.y + 2,
                8.min(i.width),
                " name: ",
                Style::default().fg(palette.subtext0).bg(palette.panel_bg),
            );
            let field = Rect::new(i.x + 7, i.y + 2, i.width.saturating_sub(8), 1);
            b.set_style(
                field,
                Style::default().fg(palette.text).bg(palette.surface0),
            );
            cursor = text_editor::render(
                b,
                field,
                &d.name,
                Style::default().fg(palette.text).bg(palette.surface0),
            );
            if let Some(error) = &d.error {
                dim_line(b, i.x, i.y + 3, i.width, &format!(" ! {error}"), palette);
            } else {
                dim_line(
                    b,
                    i.x,
                    i.y + 3,
                    i.width,
                    " ⏎ dry run — nothing is copied yet",
                    palette,
                );
            }
        }
        RestoreStep::Confirm => {
            if d.loading {
                dim_line(b, i.x, i.y + 1, i.width, " running the dry run…", palette);
            }
            let inner = i.width.saturating_sub(1).max(8) as usize;
            let mut y = i.y + 1;
            for line in wrap(&d.body, inner)
                .iter()
                .take(i.height.saturating_sub(4) as usize)
            {
                put_text(
                    b,
                    i.x,
                    y,
                    i.width,
                    &format!(" {line}"),
                    Style::default().fg(palette.text).bg(palette.panel_bg),
                );
                y += 1;
            }
            if let Some(error) = &d.error {
                dim_line(b, i.x, y, i.width, &format!(" ! {error}"), palette);
            }
        }
    }
    let rs = row(i, &[13, 12], 2, i.height.saturating_sub(2));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    match d.step {
        RestoreStep::List => {
            let style = Style::default()
                .fg(palette.text)
                .bg(palette.surface0)
                .add_modifier(Modifier::BOLD);
            button(b, *ok, " ↵ choose ", style);
        }
        RestoreStep::Name => send_button(b, *ok, " ↵ dry run ", true, palette),
        RestoreStep::Confirm => {
            let style = if d.loading || d.error.is_some() {
                Style::default().fg(palette.overlay0).bg(palette.surface0)
            } else {
                Style::default()
                    .fg(contrast(palette))
                    .bg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            };
            button(b, *ok, " ↵ restore ", style);
        }
    }
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor,
    })
}

/// Wrap text at `width` cells, one paragraph per line and blank lines
/// kept — the dry-run plan stays readable.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let word_w = usize::from(display_width(word));
            let line_w = usize::from(display_width(&line));
            if line.is_empty() {
                if word_w <= width {
                    line.push_str(word);
                } else {
                    out.push(truncate_end(word, width));
                }
            } else if line_w + 1 + word_w <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                out.push(line);
                line = word.to_owned();
            }
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
