//! The trailing new-studio frame: a dashed outline with
//! `+  new studio`, the `repository + cast` line and the ` n ` key
//! chip. Enter or a click on it opens the new-studio wizard.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

/// A style on a surface of its own.
fn on(fg: ratatui::style::Color, bg: ratatui::style::Color) -> Style {
    Style::default().fg(fg).bg(bg)
}

/// Draw the dashed frame: `overlay0` (or `accent` when selected).
pub(crate) fn draw_new(buf: &mut Buffer, rect: Rect, selected: bool, palette: &Palette) {
    if rect.width < 4 || rect.height < 3 {
        return;
    }
    let elev = palette.elevation();
    let border = if selected {
        palette.accent
    } else {
        elev.border_muted
    };
    let bg = if selected {
        elev.raised_row
    } else {
        palette.panel_bg
    };
    let edge = on(border, elev.surface);
    // The dashed outline: corners solid, the arithmetic `┄`.
    put_text(buf, rect.x, rect.y, 1, "╭", edge);
    put_text(
        buf,
        rect.x + 1,
        rect.y,
        rect.width - 2,
        &"┄".repeat((rect.width - 2) as usize),
        edge,
    );
    put_text(buf, rect.x + rect.width - 1, rect.y, 1, "╮", edge);
    for y in rect.y + 1..rect.y + rect.height - 1 {
        put_text(buf, rect.x, y, 1, "┆", edge);
        put_text(buf, rect.x + rect.width - 1, y, 1, "┆", edge);
        for x in rect.x + 1..rect.x + rect.width - 1 {
            buf[(x, y)].set_symbol(" ");
            buf[(x, y)].set_style(on(border, bg));
        }
    }
    let bottom = rect.y + rect.height - 1;
    put_text(buf, rect.x, bottom, 1, "╰", edge);
    put_text(
        buf,
        rect.x + 1,
        bottom,
        rect.width - 2,
        &"┄".repeat((rect.width - 2) as usize),
        edge,
    );
    put_text(buf, rect.x + rect.width - 1, bottom, 1, "╯", edge);

    // Centred: the title, the cast note and the key chip.
    let inner = rect.width.saturating_sub(2);
    let centre = |text: &str| rect.x + 1 + inner.saturating_sub(text.chars().count() as u16) / 2;
    let title = truncate_end("+  new studio", inner as usize);
    put_text(
        buf,
        centre(&title),
        rect.y + 2,
        title.chars().count() as u16,
        &title,
        on(palette.accent, bg).add_modifier(Modifier::BOLD),
    );
    let note = truncate_end("repository + cast", inner as usize);
    put_text(
        buf,
        centre(&note),
        rect.y + 3,
        note.chars().count() as u16,
        &note,
        on(elev.muted_surface, bg),
    );
    if inner >= 3 {
        let chip_bg = if selected || elev.is_dark {
            palette.surface0
        } else {
            elev.surface
        };
        put_text(
            buf,
            centre(" n "),
            rect.y + 5,
            3,
            " n ",
            on(palette.accent, chip_bg).add_modifier(Modifier::BOLD),
        );
    }
}
