// Derived from herdr src/client/shell/notifications.rs (Apache-2.0).
//! The reference mobile banner and endpoint/notification toast cards.

use super::palette::Palette;
use crate::app::notifications::{Notification, NotificationKind};
use crate::config::ToastHerdrPosition;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

pub(crate) fn render_mobile_notice_banner(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    body: Option<&str>,
    dot_color: Color,
    offset_for_warning: bool,
    palette: &Palette,
) -> Rect {
    if area.is_empty() {
        return Rect::default();
    }
    let warning_offset = u16::from(offset_for_warning);
    let y = area.y
        + area
            .height
            .saturating_sub(1u16.saturating_add(warning_offset));
    let rect = Rect::new(area.x, y, area.width, 1);
    let background = palette.surface0;
    Clear.render(rect, buffer);
    buffer.set_style(rect, Style::default().bg(background));
    let mut x = rect.x;
    for (text, style) in [
        (" ", Style::default().bg(background)),
        ("●", Style::default().fg(dot_color).bg(background)),
        (" ", Style::default().bg(background)),
        (
            title,
            Style::default()
                .fg(palette.text)
                .bg(background)
                .add_modifier(Modifier::BOLD),
        ),
    ] {
        x = super::render::put_segment(buffer, x, rect.y, rect.right(), text, style);
    }
    if let Some(body) = body.filter(|body| !body.is_empty()) {
        x = super::render::put_segment(
            buffer,
            x,
            rect.y,
            rect.right(),
            " · ",
            Style::default().fg(palette.overlay0).bg(background),
        );
        super::render::put_text(
            buffer,
            x,
            rect.y,
            rect.right().saturating_sub(x),
            body,
            Style::default().fg(palette.overlay0).bg(background),
        );
    }
    rect
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_notification_card(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    body: &str,
    position: ToastHerdrPosition,
    top_offset: u16,
    dot_color: Color,
    palette: &Palette,
) -> Rect {
    if area.is_empty() {
        return Rect::default();
    }
    let content_width = unicode_width::UnicodeWidthStr::width(title)
        .max(unicode_width::UnicodeWidthStr::width(body))
        .saturating_add(6);
    let width = u16::try_from(content_width)
        .unwrap_or(u16::MAX)
        .min(area.width);
    let height: u16 = if body.is_empty() { 3 } else { 4 }.min(area.height);
    let x = match position {
        ToastHerdrPosition::TopLeft | ToastHerdrPosition::BottomLeft => area.x,
        ToastHerdrPosition::TopRight | ToastHerdrPosition::BottomRight => {
            area.right().saturating_sub(width)
        }
    };
    let max_y = area.bottom().saturating_sub(height).max(area.y);
    let y = match position {
        ToastHerdrPosition::TopLeft | ToastHerdrPosition::TopRight => {
            area.y.saturating_add(top_offset)
        }
        ToastHerdrPosition::BottomLeft | ToastHerdrPosition::BottomRight => area
            .bottom()
            .saturating_sub(height.saturating_add(top_offset)),
    }
    .clamp(area.y, max_y);
    let rect = Rect::new(x, y, width, height);
    Clear.render(rect, buffer);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.overlay0))
        .style(Style::default().bg(palette.panel_bg));
    let inner = block.inner(rect);
    block.render(rect, buffer);
    Paragraph::new(Line::from(vec![
        Span::styled("●", Style::default().fg(dot_color)),
        Span::raw(" "),
        Span::styled(
            title,
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .render(Rect::new(inner.x, inner.y, inner.width, 1), buffer);
    if !body.is_empty() && inner.height > 1 {
        Paragraph::new(Line::from(Span::styled(
            body,
            Style::default().fg(palette.overlay0),
        )))
        .render(
            Rect::new(
                inner.x.saturating_add(2),
                inner.y + 1,
                inner.width.saturating_sub(2),
                1,
            ),
            buffer,
        );
    }
    rect
}

/// The reference `render_visible_notification`: the toast card at the
/// configured corner. Returns its rect for click-to-focus hits.
pub(crate) fn render_visible_notification(
    buffer: &mut Buffer,
    area: Rect,
    notification: &Notification,
    position: ToastHerdrPosition,
    top_offset: u16,
    palette: &Palette,
) -> Rect {
    let event = &notification.event;
    let dot_color = match event.kind {
        NotificationKind::NeedsAttention => palette.red,
        NotificationKind::Finished => palette.blue,
    };
    render_notification_card(
        buffer,
        area,
        &event.title,
        event.body.as_deref().unwrap_or_default(),
        position,
        top_offset,
        dot_color,
        palette,
    )
}

/// The reference `render_mobile_notification_banner`: one bottom row with
/// the short "waiting"/"done" title rewrite.
pub(crate) fn render_mobile_notification_banner(
    buffer: &mut Buffer,
    area: Rect,
    notification: &Notification,
    offset_for_warning: bool,
    palette: &Palette,
) -> Rect {
    let event = &notification.event;
    let title = match event.kind {
        NotificationKind::NeedsAttention => event
            .title
            .strip_suffix(" needs attention")
            .map(|agent| format!("{agent} waiting"))
            .unwrap_or_else(|| event.title.clone()),
        NotificationKind::Finished => event
            .title
            .strip_suffix(" finished")
            .map(|agent| format!("{agent} done"))
            .unwrap_or_else(|| event.title.clone()),
    };
    let dot_color = match event.kind {
        NotificationKind::NeedsAttention => palette.red,
        NotificationKind::Finished => palette.blue,
    };
    render_mobile_notice_banner(
        buffer,
        area,
        &title,
        event.body.as_deref(),
        dot_color,
        offset_for_warning,
        palette,
    )
}
