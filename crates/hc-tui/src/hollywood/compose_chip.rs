//! The compose line's fixed target chip and its narrow-width ladder.

use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;

/// Room the ladder tries to leave for the draft after chip + ` │ `.
const MIN_TEXT_CELLS: u16 = 12;

pub(super) struct ComposeChip {
    pub text: String,
    pub reply: bool,
}

fn fits(label: &str, span: u16) -> bool {
    (display_width(label) as u16) + 3 + MIN_TEXT_CELLS <= span
}

/// The chip for the compose target. A reply never disappears: at the
/// narrowest it is still its `↩` glyph.
pub(super) fn compose_chip(
    reply_to: Option<&(crate::studio::StudioKey, String)>,
    span: u16,
) -> ComposeChip {
    let Some((studio, role)) = reply_to else {
        return ComposeChip {
            text: "hollywood".into(),
            reply: false,
        };
    };
    let full = format!("↩ {} · {}", studio.id, role);
    if fits(&full, span) {
        return ComposeChip {
            text: full,
            reply: true,
        };
    }
    let role_only = format!("↩ {role}");
    if fits(&role_only, span) {
        return ComposeChip {
            text: role_only,
            reply: true,
        };
    }
    ComposeChip {
        text: "↩".into(),
        reply: true,
    }
}

/// Draw chip + separator, returning the text field's left edge. The
/// separator reads as part of the chip; both use gilt for Hollywood and
/// teal for a reply.
pub(super) fn draw_compose_chip(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    reply_to: Option<&(crate::studio::StudioKey, String)>,
    palette: &Palette,
) -> u16 {
    let span = right.saturating_sub(x);
    if span == 0 {
        return right;
    }
    let chip = compose_chip(reply_to, span);
    let field_bg = palette.elevation().raised_field;
    let style = if chip.reply {
        Style::default().fg(palette.teal).bg(field_bg)
    } else {
        palette.elevation().gilt_style(field_bg)
    };
    let chip_w = (display_width(&chip.text) as u16).min(span);
    put_text(buf, x, y, chip_w, &chip.text, style);
    let after = x + chip_w;
    if after + 2 < right {
        put_text(buf, after + 1, y, 1, "│", style);
        return (after + 3).min(right);
    }
    after.min(right)
}
