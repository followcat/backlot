// Derived from herdr src/ui/status.rs `copy_feedback_rect`,
// `render_copy_feedback_buffer` and src/ui.rs
// `copy_feedback_offset_for_toast` (Apache-2.0).
//! The `copied to clipboard` card: a green-bordered toast anchored over
//! the pane surface for two seconds after a successful copy.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::chrome::palette::Palette;
use crate::config::ToastClipboardPosition;

/// The feedback card's rect inside `area` — `content` plus a cell of
/// padding and the border on each side.
pub(crate) fn copy_feedback_rect(
    area: Rect,
    message: &str,
    offset_rows: u16,
    position: ToastClipboardPosition,
) -> Rect {
    if area.is_empty() {
        return Rect::default();
    }
    let content_width = message.len() as u16 + 4;
    let width = content_width.min(area.width);
    let height = 3u16.min(area.height);
    let x = match position {
        ToastClipboardPosition::TopLeft | ToastClipboardPosition::BottomLeft => area.x,
        ToastClipboardPosition::TopCenter | ToastClipboardPosition::BottomCenter => {
            area.x + area.width.saturating_sub(width) / 2
        }
        ToastClipboardPosition::TopRight | ToastClipboardPosition::BottomRight => {
            area.x + area.width.saturating_sub(width)
        }
    };
    let y = match position {
        ToastClipboardPosition::TopLeft
        | ToastClipboardPosition::TopCenter
        | ToastClipboardPosition::TopRight => area.y + offset_rows.min(area.height),
        ToastClipboardPosition::BottomLeft
        | ToastClipboardPosition::BottomCenter
        | ToastClipboardPosition::BottomRight => {
            area.y + area.height.saturating_sub(height + offset_rows)
        }
    };
    Rect::new(x, y, width, height)
}

/// The reference's `copy_feedback_offset_for_toast`: when the card
/// would overlap the notification toast it slides below it.
pub(crate) fn copy_feedback_offset_for_toast(
    area: Rect,
    message: &str,
    base_offset: u16,
    position: ToastClipboardPosition,
    toast_rect: Rect,
) -> u16 {
    let feedback_rect = copy_feedback_rect(area, message, base_offset, position);
    if rectangles_overlap(feedback_rect, toast_rect) {
        base_offset.saturating_add(toast_rect.height)
    } else {
        base_offset
    }
}

fn rectangles_overlap(left: Rect, right: Rect) -> bool {
    left.x < right.right()
        && right.x < left.right()
        && left.y < right.bottom()
        && right.y < left.bottom()
}

/// Clear `area`, draw the bordered card and the `● copied…` line.
/// Returns the painted rect — empty when the area cannot fit it.
pub(crate) fn render_copy_feedback_buffer(
    buffer: &mut Buffer,
    area: Rect,
    message: &str,
    offset_rows: u16,
    position: ToastClipboardPosition,
    palette: &Palette,
) -> Rect {
    let feedback_area = copy_feedback_rect(area, message, offset_rows, position);
    if feedback_area.is_empty() {
        return feedback_area;
    }

    Clear.render(feedback_area, buffer);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.green))
        .style(Style::default().bg(palette.panel_bg));
    let inner = block.inner(feedback_area);
    block.render(feedback_area, buffer);

    if inner.height == 0 {
        return feedback_area;
    }

    let text = Line::from(vec![
        Span::styled("●", Style::default().fg(palette.green).bg(palette.panel_bg)),
        Span::raw(" "),
        Span::styled(
            message,
            Style::default()
                .fg(palette.text)
                .bg(palette.panel_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    Paragraph::new(text).render(inner, buffer);
    feedback_area
}
