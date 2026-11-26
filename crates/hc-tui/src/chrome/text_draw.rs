//! The shared cell painters: left text, right-aligned text, a clipped
//! segment, and the role-badge helper every surface uses.

use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthStr;

use super::palette::Palette;

pub(crate) fn put_right_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let width = display_width(text).min(area.width);
    put_text(
        buffer,
        area.right().saturating_sub(width),
        y,
        width,
        text,
        style,
    );
}

pub(crate) fn put_segment(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    text: &str,
    style: Style,
) -> u16 {
    let width = display_width(text).min(right.saturating_sub(x));
    put_text(buffer, x, y, width, text, style);
    x.saturating_add(width)
}

pub(crate) fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if width == 0 || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    buffer.set_stringn(x, y, text, width as usize, style);
}

/// Draw `text`, giving every role badge (`Ⓟ Ⓓ Ⓦ Ⓐ`) its role's colour
/// and the rest `style`. The one helper every surface uses, so a
/// fallback glyph set could be added in a single place. Returns the
/// cells written.
pub(crate) fn put_role_badged_text(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    style: Style,
    palette: &Palette,
) -> u16 {
    let mut cursor = x;
    let mut left = width;
    let mut drawn = 0u16;
    let mut run = String::new();
    for ch in text.chars() {
        let Some(badge) = crate::studio::glyphs::RoleBadge::of_glyph(ch) else {
            run.push(ch);
            continue;
        };
        if !run.is_empty() {
            let take = display_width(&run).min(left);
            put_text(buffer, cursor, y, take, &run, style);
            cursor += take;
            left -= take;
            drawn += take;
            run.clear();
            if left == 0 {
                return drawn;
            }
        }
        put_text(
            buffer,
            cursor,
            y,
            1,
            badge.glyph(),
            style.fg(badge.color(palette)),
        );
        cursor += 1;
        left -= 1;
        drawn += 1;
        if left == 0 {
            return drawn;
        }
    }
    if !run.is_empty() {
        let take = display_width(&run).min(left);
        put_text(buffer, cursor, y, take, &run, style);
        drawn += take;
    }
    drawn
}

pub(super) fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}
