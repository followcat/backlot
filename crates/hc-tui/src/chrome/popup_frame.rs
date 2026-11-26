//! Unified modal & menu popup chrome: backdrop dimming, shrink-ladder
//! geometry (`gutter -> padding -> content`), rounded frame (`╭╮╰╯`),
//! 1-cell drop shadow, and pre-draw CJK seam cleanup.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use super::palette::Palette;
use super::wide::{blank_wide_edges, clear_overlay};
use crate::config::PopupBackdrop;

pub(crate) use super::popup_geometry::{
    PopupChrome, PopupGeometry, resolve_centered, resolve_from_border_size, wrap_border_in_host,
};

/// Elevated modal background color derived from `palette`.
#[inline]
pub(crate) fn overlay_bg(palette: &Palette) -> Color {
    palette.elevation().overlay
}

/// Contrast foreground for solid `palette.accent` / `palette.red` buttons.
#[inline]
pub(crate) fn contrast(palette: &Palette) -> Color {
    match overlay_bg(palette) {
        Color::Reset => Color::Black,
        bg => bg,
    }
}

/// Dim every cell inside `region` (intersected with `buf.area`), skipping `exclude`.
pub(crate) fn dim_backdrop_in(
    buf: &mut Buffer,
    region: Rect,
    palette: &Palette,
    mode: PopupBackdrop,
    exclude: Option<Rect>,
) {
    if mode == PopupBackdrop::None {
        return;
    }
    let clipped = region.intersection(buf.area);
    if clipped.is_empty() {
        return;
    }
    let elev = palette.elevation();
    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            if exclude.is_some_and(|ex| ex.contains(Position::new(x, y))) {
                continue;
            }
            elev.dim_cell(&mut buf[(x, y)], palette);
        }
    }
}

/// Dim the entire buffer except `exclude`.
#[inline]
pub(crate) fn dim_backdrop(
    buf: &mut Buffer,
    palette: &Palette,
    mode: PopupBackdrop,
    exclude: Option<Rect>,
) {
    dim_backdrop_in(buf, buf.area, palette, mode, exclude);
}

/// Apply a translucent drop shadow across a 1-row strip (used by popups'
/// bottom shadow and Call Sheet bottom shadow), guarding CJK seams first.
pub(crate) fn draw_bottom_shadow(
    buf: &mut Buffer,
    row_span: Rect,
    clip: Rect,
    palette: &Palette,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    let elev = palette.elevation();
    if elev.is_terminal_16 {
        return;
    }
    let clipped = row_span.intersection(clip).intersection(buf.area);
    if clipped.is_empty() {
        return;
    }
    blank_wide_edges(buf, clipped, true);
    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            elev.shadow_cell(&mut buf[(x, y)], palette);
        }
    }
}

/// Draw the classic 1-cell drop shadow one column right (`outer.right()`) and
/// one row below (`outer.bottom()`), offset by `(1, 1)` with no corner double-dim.
pub(crate) fn draw_drop_shadow(
    buf: &mut Buffer,
    outer: Rect,
    clip: Rect,
    palette: &Palette,
    enabled: bool,
) {
    if !enabled || outer.is_empty() {
        return;
    }
    let elev = palette.elevation();
    if elev.is_terminal_16 {
        return;
    }
    let right_strip = Rect::new(
        outer.right(),
        outer.y.saturating_add(1),
        1,
        outer.height.saturating_sub(1),
    )
    .intersection(clip)
    .intersection(buf.area);
    if !right_strip.is_empty() {
        blank_wide_edges(buf, right_strip, true);
        for y in right_strip.top()..right_strip.bottom() {
            for x in right_strip.left()..right_strip.right() {
                elev.shadow_cell(&mut buf[(x, y)], palette);
            }
        }
    }

    let bottom_strip = Rect::new(outer.x.saturating_add(1), outer.bottom(), outer.width, 1);
    draw_bottom_shadow(buf, bottom_strip, clip, palette, enabled);
}

/// Sanitize CJK seams, clear & fill `geom.outer`, draw rounded border `╭╮╰╯`
/// on `geom.border`, and draw the drop shadow clipped to `clip`.
pub(crate) fn paint_frame_clipped(
    buf: &mut Buffer,
    geom: PopupGeometry,
    clip: Rect,
    border_fg: Color,
    chrome: PopupChrome,
    palette: &Palette,
    shadow: bool,
) -> Option<PopupGeometry> {
    if geom.border.width < 2 || geom.border.height < 2 {
        return None;
    }
    let outer = geom.outer.intersection(buf.area);
    let border = geom.border.intersection(buf.area);
    if border.width < 2 || border.height < 2 {
        return None;
    }
    clear_overlay(buf, outer);

    let bg = overlay_bg(palette);
    let background = Style::default().bg(bg).remove_modifier(Modifier::DIM);
    let border_style = Style::default()
        .fg(border_fg)
        .bg(bg)
        .remove_modifier(Modifier::DIM);

    match chrome {
        PopupChrome::Terminal => {
            for y in outer.y..outer.bottom() {
                for x in outer.x..outer.right() {
                    if !geom.inner.contains(Position::new(x, y)) {
                        buf[(x, y)].set_symbol(" ").set_style(background);
                    }
                }
            }
            super::overlay::fill_default_bg(buf, geom.inner.intersection(buf.area), palette);
        }
        PopupChrome::Standard | PopupChrome::BorderJoined | PopupChrome::Menu => {
            for y in outer.y..outer.bottom() {
                for x in outer.x..outer.right() {
                    buf[(x, y)].set_symbol(" ").set_style(background);
                }
            }
        }
    }

    draw_rounded_border(buf, border, border_style);
    draw_drop_shadow(buf, outer, clip, palette, shadow);
    Some(geom)
}

#[inline]
pub(crate) fn paint_frame(
    buf: &mut Buffer,
    geom: PopupGeometry,
    border_fg: Color,
    chrome: PopupChrome,
    palette: &Palette,
    shadow: bool,
) -> Option<PopupGeometry> {
    paint_frame_clipped(buf, geom, buf.area, border_fg, chrome, palette, shadow)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_centered(
    buf: &mut Buffer,
    host: Rect,
    content_w: u16,
    content_h: u16,
    min_border_w: u16,
    min_border_h: u16,
    border_fg: Color,
    chrome: PopupChrome,
    palette: &Palette,
    shadow: bool,
) -> Option<PopupGeometry> {
    let geom = resolve_centered(
        host,
        content_w,
        content_h,
        min_border_w,
        min_border_h,
        chrome,
    )?;
    paint_frame(buf, geom, border_fg, chrome, palette, shadow)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_from_border_size(
    buf: &mut Buffer,
    host: Rect,
    border_w: u16,
    border_h: u16,
    min_border_w: u16,
    min_border_h: u16,
    border_fg: Color,
    chrome: PopupChrome,
    palette: &Palette,
    shadow: bool,
) -> Option<PopupGeometry> {
    let geom =
        resolve_from_border_size(host, border_w, border_h, min_border_w, min_border_h, chrome)?;
    paint_frame(buf, geom, border_fg, chrome, palette, shadow)
}

fn draw_rounded_border(buf: &mut Buffer, b: Rect, style: Style) {
    let top = b.y;
    let bot = b.bottom() - 1;
    for x in b.x..b.right() {
        buf[(x, top)]
            .set_symbol(if x == b.x {
                "╭"
            } else if x + 1 == b.right() {
                "╮"
            } else {
                "─"
            })
            .set_style(style);
        buf[(x, bot)]
            .set_symbol(if x == b.x {
                "╰"
            } else if x + 1 == b.right() {
                "╯"
            } else {
                "─"
            })
            .set_style(style);
    }
    for y in b.y + 1..bot {
        buf[(b.x, y)].set_symbol("│").set_style(style);
        buf[(b.right() - 1, y)].set_symbol("│").set_style(style);
    }
}
