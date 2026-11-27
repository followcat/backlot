// Derived from herdr src/client/shell/notifications.rs (Apache-2.0).
//! The reference mobile banner and endpoint/notification toast cards.

use super::palette::Palette;
use super::render::put_text;
use super::text::truncate_end;
use crate::app::notifications::{Notification, NotificationKind};
use crate::config::ToastHerdrPosition;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

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
    super::wide::clear_overlay(buffer, rect);
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
    super::wide::clear_overlay(buffer, rect);
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
    // The release-notes card must be opaque in backlot's own look.
    super::overlay::fill_default_bg(buffer, rect, palette);
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

/// The action toast's chip — the failure card's `copy cmd` or the
/// stopped-stage offer's `start stage` (R3 §1b).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastChip {
    /// `[C] copy cmd` — the failure's exact command through OSC 52.
    CopyCmd,
    /// `[s] start stage` — `backlot stage start` on the recorded
    /// machine while the stage is stopped.
    StartStage,
}

impl ToastChip {
    /// The chip text as drawn.
    pub(crate) fn text(self) -> &'static str {
        match self {
            ToastChip::CopyCmd => "[C] copy cmd",
            ToastChip::StartStage => "[s] start stage",
        }
    }
}

/// The S3 action toast (§3): a success card is the title alone and
/// fades after 3.5 s; a body plus a chip holds 8 s — the failure
/// card's `stderr` line and `[C] copy cmd`, or the stopped-stage
/// offer's `recorded on disk` line and `[s] start stage`, which
/// keeps the success card's neutral border and green dot. Returns
/// the card rect and the chip's hit rect.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_action_toast(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    body: Option<&str>,
    chip: Option<ToastChip>,
    top_offset: u16,
    palette: &Palette,
    deco: bool,
) -> (Rect, Rect) {
    if deco {
        return super::toast_deco::render_action_toast_deco(
            buffer, area, title, body, chip, top_offset, palette,
        );
    }
    if area.is_empty() {
        return (Rect::default(), Rect::default());
    }
    let fail = chip == Some(ToastChip::CopyCmd);
    let hint = chip.map(|c| format!("{}  [esc] dismiss", c.text()));
    let content_width = UnicodeWidthStr::width(title)
        .max(body.map_or(0, UnicodeWidthStr::width))
        .max(hint.as_deref().map_or(0, UnicodeWidthStr::width))
        .saturating_add(6);
    let width = u16::try_from(content_width)
        .unwrap_or(u16::MAX)
        .min(area.width);
    let lines: u16 = match body {
        Some(_) => 3,
        None => 1,
    };
    let height = (lines + 2).min(area.height);
    let x = area.right().saturating_sub(width);
    let max_y = area.bottom().saturating_sub(height).max(area.y);
    let y = area.y.saturating_add(top_offset).clamp(area.y, max_y);
    let rect = Rect::new(x, y, width, height);
    super::wide::clear_overlay(buffer, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if fail { palette.red } else { palette.overlay0 }))
        .style(Style::default().bg(palette.panel_bg));
    let inner = block.inner(rect);
    block.render(rect, buffer);
    let dot = if fail { palette.red } else { palette.green };
    Paragraph::new(Line::from(vec![
        Span::styled("●", Style::default().fg(dot)),
        Span::raw(" "),
        Span::styled(
            truncate_end(title, inner.width.saturating_sub(4) as usize),
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .render(Rect::new(inner.x, inner.y, inner.width, 1), buffer);
    let mut chip_hit = Rect::default();
    if let Some(body) = body {
        if inner.height > 1 {
            Paragraph::new(Line::from(Span::styled(
                truncate_end(body, inner.width.saturating_sub(4) as usize),
                Style::default().fg(palette.overlay0),
            )))
            .render(
                Rect::new(
                    inner.x.saturating_add(2),
                    inner.y + 1,
                    inner.width.saturating_sub(4),
                    1,
                ),
                buffer,
            );
        }
        if inner.height > 2
            && let Some(chip) = chip
        {
            // The chip is the click target — the rest of the
            // hint is inert text so it stays unambiguous (§3.3).
            let hint_y = inner.y + 2;
            let chip_text = chip.text();
            let chip_w = UnicodeWidthStr::width(chip_text) as u16;
            put_text(
                buffer,
                inner.x + 2,
                hint_y,
                inner.width.saturating_sub(2),
                chip_text,
                Style::default().fg(palette.teal),
            );
            put_text(
                buffer,
                inner.x + 2 + chip_w,
                hint_y,
                inner.width.saturating_sub(2 + chip_w),
                "  [esc] dismiss",
                Style::default().fg(palette.overlay0),
            );
            chip_hit = Rect::new(inner.x + 2, hint_y, chip_w.min(inner.width), 1);
        }
    }
    super::overlay::fill_default_bg(buffer, rect, palette);
    (rect, chip_hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release-notes card (`Update available` …) is an overlay over
    /// the panes: in backlot's own look every cell inside its frame is
    /// opaque, so the terminal behind never shows through.
    #[test]
    fn the_release_notes_card_is_opaque_in_the_backlot_theme() {
        let area = Rect::new(0, 0, 60, 20);
        let mut buffer = Buffer::empty(area);
        let rect = render_notification_card(
            &mut buffer,
            area,
            "Update available",
            "backlot v9.9.9 is out — run: backlot --upgrade",
            ToastHerdrPosition::TopRight,
            1,
            Color::Red,
            &Palette::backlot(),
        );
        assert!(!rect.is_empty());
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                assert_ne!(
                    buffer[(x, y)].bg,
                    Color::Reset,
                    "see-through release card cell at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn action_toast_preserves_right_border_and_padding_when_clamped_to_area() {
        let area = Rect::new(4, 11, 96, 7);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 18));
        let body = "switch to codex/gpt-6-sol/high failed: candidate codex/gpt-6-sol hit quota limit — kept claude/sonnet-5-5/high live";
        let (rect, _chip) = render_action_toast(
            &mut buffer,
            area,
            "hollywood switch failed",
            Some(body),
            Some(ToastChip::CopyCmd),
            0,
            &Palette::backlot(),
            true,
        );
        assert_eq!(rect.right(), 100);
        for dy in 1..rect.height.saturating_sub(1) {
            let y = rect.y + dy;
            assert_eq!(buffer[(rect.x, y)].symbol(), "║", "left border row {y}");
            assert_eq!(
                buffer[(rect.right() - 1, y)].symbol(),
                "║",
                "right border row {y}"
            );
            assert_eq!(
                buffer[(rect.right() - 2, y)].symbol(),
                " ",
                "right inner margin row {y}"
            );
        }
    }
}
