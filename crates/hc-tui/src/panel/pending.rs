//! The ActionTracker's in-flight mark on a dock row (S3 §3.1):
//! `⠋ <label>…` dimmed at the row's right edge while its command
//! runs, inside the block border on the bordered row kinds.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::rows::{Row, Target};
use crate::action::ActionTracker;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;

/// Draw the pending mark for `row`'s target when its mark is in
/// flight. `spin` is the dock's frame counter; `dim` the dim style.
/// Returns `true` when a mark was drawn — the caller flags it so the
/// spinner keeps animating.
pub(super) fn mark(
    buf: &mut Buffer,
    rect: Rect,
    row: &Row,
    target: &Target,
    pending: &ActionTracker,
    spin: usize,
    dim: Style,
) -> bool {
    let Some(mark) = crate::action::mark::of(target) else {
        return false;
    };
    let Some(text) = pending.pending_label(&mark) else {
        return false;
    };
    let inner_width = if matches!(
        row,
        Row::Person { .. } | Row::Task { .. } | Row::AcceptedFold { .. }
    ) {
        rect.width.saturating_sub(2)
    } else {
        rect.width
    };
    let mark_x = if inner_width > 1 { rect.x + 1 } else { rect.x };
    let w = display_width(text) as u16 + 3;
    if inner_width <= w {
        return false;
    }
    let glyph = crate::studio::glyphs::Person::Working.glyph(spin);
    put_text(
        buf,
        mark_x + inner_width - w,
        rect.y,
        w,
        &format!("{glyph} {text}"),
        dim,
    );
    true
}
