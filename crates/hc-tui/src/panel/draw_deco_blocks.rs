//! Three-weight studio block borders and finished top-border tails for Babylon Lot (S4 §2–§3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::data::StudioRef;
use super::{Panel, Target};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::studio::StudioKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockWeight {
    Resting,
    Selected,
    Entered,
}

impl BlockWeight {
    pub(super) fn of(panel: &Panel, lot_focused: bool, studio: &StudioKey) -> Self {
        if !lot_focused {
            return Self::Resting;
        }
        if panel.entered_studio.as_ref() == Some(studio) {
            return Self::Entered;
        }
        if !panel.plain_chrome
            && panel.entered_studio.is_none()
            && matches!(&panel.selected, Some(Target::Studio(k)) if k == studio)
        {
            return Self::Selected;
        }
        Self::Resting
    }

    pub(super) fn top_corners(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Resting => ("╭", "─", "╮"),
            Self::Selected => ("╔", "═", "╗"),
            Self::Entered => ("┏", "━", "┓"),
        }
    }

    pub(super) fn bottom_corners(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Resting => ("╰", "─", "╯"),
            Self::Selected => ("╚", "═", "╝"),
            Self::Entered => ("┗", "━", "┛"),
        }
    }

    pub(super) fn divider_ends(self) -> (&'static str, &'static str) {
        match self {
            Self::Resting => ("├", "┤"),
            Self::Selected => ("╟", "╢"),
            Self::Entered => ("┣", "┫"),
        }
    }

    pub(super) fn side_sym(self) -> &'static str {
        match self {
            Self::Resting => "│",
            Self::Selected => "║",
            Self::Entered => "┃",
        }
    }
}

pub(super) fn frame_style(focused_selected: bool, plain_chrome: bool, palette: &Palette) -> Style {
    let elev = palette.elevation();
    if focused_selected {
        Style::default().fg(palette.accent)
    } else if plain_chrome {
        Style::default().fg(elev.border_muted)
    } else {
        let mut s = Style::default().fg(elev.gilt);
        if elev.is_terminal_16 {
            s = s.add_modifier(Modifier::DIM);
        }
        s
    }
}

pub(super) fn draw_block_bottom(buf: &mut Buffer, rect: Rect, weight: BlockWeight, frame: Style) {
    if rect.width == 0 {
        return;
    }
    let (left, fill, right) = weight.bottom_corners();
    put_text(buf, rect.x, rect.y, 1, left, frame);
    if rect.width > 2 {
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            rect.width.saturating_sub(2),
            &fill.repeat(rect.width.saturating_sub(2) as usize),
            frame,
        );
    }
    if rect.width > 1 {
        put_text(buf, rect.x + rect.width - 1, rect.y, 1, right, frame);
    }
}

pub(super) fn draw_divider(
    buf: &mut Buffer,
    rect: Rect,
    weight: BlockWeight,
    frame: Style,
    rule_style: Option<Style>,
) {
    if rect.width < 2 {
        return;
    }
    let (left, right) = weight.divider_ends();
    put_text(buf, rect.x, rect.y, 1, left, frame);
    if rect.width > 2 {
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            rect.width - 2,
            &"─".repeat((rect.width - 2) as usize),
            rule_style.unwrap_or(frame),
        );
    }
    put_text(buf, rect.x + rect.width - 1, rect.y, 1, right, frame);
}

pub(super) fn stamp_block_sides(
    buf: &mut Buffer,
    rect: Rect,
    weight: BlockWeight,
    palette: &Palette,
) {
    if weight == BlockWeight::Resting || rect.width == 0 {
        return;
    }
    let style = Style::default()
        .fg(palette.accent)
        .bg(palette.elevation().surface);
    let sym = weight.side_sym();
    put_text(buf, rect.x, rect.y, 1, sym, style);
    if rect.width > 1 {
        put_text(buf, rect.x + rect.width - 1, rect.y, 1, sym, style);
    }
}

/// Finished studio top border tail (`✦ THE END·12/12` → `✦ END` → `✓ done`),
/// or `None` when not finished or in plain/herdr mode.
pub(super) fn finished_top_tail(
    studio: StudioRef<'_>,
    rect_width: u16,
    avail: u16,
    label_full_w: u16,
    plain_chrome: bool,
) -> Option<(String, bool)> {
    if plain_chrome || studio.studio.state != "finished" || studio.studio.ready_to_finish {
        return None;
    }
    let full = format!(
        "✦ THE END·{}/{}",
        studio.studio.tasks_done, studio.studio.tasks_total
    );
    let full_w = display_width(&full) as u16;
    if rect_width >= 32 && label_full_w.min(10) + full_w <= avail {
        return Some((full, true));
    }
    if rect_width >= 24 && display_width("✦ END") as u16 + 4 <= avail {
        return Some(("✦ END".to_string(), true));
    }
    if display_width("✓ done") as u16 + 4 <= avail {
        return Some(("✓ done".to_string(), false));
    }
    Some(("✓".to_string(), false))
}

pub(super) fn paint_finished_tail(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tail: &str,
    deco_colors: bool,
    fallback_fg: Color,
    palette: &Palette,
) {
    if !deco_colors {
        put_text(
            buf,
            x,
            y,
            display_width(tail) as u16,
            tail,
            Style::default().fg(fallback_fg),
        );
        return;
    }
    let elev = palette.elevation();
    let mut gilt = Style::default().fg(elev.gilt);
    if elev.is_terminal_16 {
        gilt = gilt.add_modifier(Modifier::DIM);
    }
    if let Some(rest) = tail.strip_prefix("✦") {
        put_text(buf, x, y, 1, "✦", Style::default().fg(palette.accent));
        let rw = display_width(rest) as u16;
        if rw > 0 {
            put_text(buf, x + 1, y, rw, rest, gilt);
        }
    } else {
        put_text(buf, x, y, display_width(tail) as u16, tail, gilt);
    }
}
