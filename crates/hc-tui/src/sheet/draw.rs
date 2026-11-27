//! The sheet's draw pass: a dimmed full-buffer modal centred in the
//! band `App` hands it — `modal_band()`, the frame minus a beside
//! dock, so the box never crosses the dock's seam (A6.2). The frame
//! is the shared rounded-box vocabulary the other dialogs use; the
//! tab row, the selection and the footer read the dock's own hints.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::lines::{Row, lines_for};
use super::{Sheet, SheetTab};
use crate::chrome::intertitle::{Flank, Part, draw_intertitle, intertitle};
use crate::chrome::palette::Palette;
use crate::chrome::popup_frame::{self, PopupChrome, overlay_bg};
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// What a render leaves for the caller — the modal's rect, for the
/// click-outside test and the tests' geometry assertions.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SheetRender {
    pub frame: Rect,
    /// The content rows' rect — where `j`/`k` scrolling applies.
    pub body: Rect,
}

/// The sheet's box inside `band`: 92×28 at 160+, 86×28 at 100+,
/// 76×20 at 64+, 58×20 at 50+, else 38×20 — the spec's sizes, each
/// clamped to the band so a narrow frame never overflows (S2 §3).
pub(crate) fn sheet_rect(band: Rect) -> Rect {
    let width = match band.width {
        140.. => 92,
        100..=139 => 86,
        64..=99 => 76,
        50..=63 => 58,
        _ => 38,
    }
    .min(band.width);
    let height = if band.height >= 36 { 28 } else { 20 }.min(band.height);
    Rect::new(
        band.x + band.width.saturating_sub(width) / 2,
        band.y + band.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// The line's colour for its tone — the dock's own words.
fn row_style(row: Row, palette: &Palette) -> Style {
    let bg = overlay_bg(palette);
    match row {
        Row::Text => Style::default().fg(palette.text).bg(bg),
        Row::Head => Style::default()
            .fg(palette.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
        Row::Dim => Style::default().fg(palette.overlay0).bg(bg),
        Row::Good => Style::default().fg(palette.green).bg(bg),
        Row::Bad => Style::default().fg(palette.red).bg(bg),
    }
}

/// Draw the whole sheet: dim the buffer (already done by the caller
/// like the other modals), the rounded frame, the studio/task title,
/// the three tabs, the scrolled content and the footer.
#[cfg(test)]
pub(crate) fn render(
    buf: &mut Buffer,
    band: Rect,
    sheet: &Sheet,
    summary: Option<&backlot_model::summary::StudioSummary>,
    palette: &Palette,
) -> SheetRender {
    render_with_shadow(buf, band, sheet, summary, palette, true)
}

pub(crate) fn render_with_shadow(
    buf: &mut Buffer,
    band: Rect,
    sheet: &Sheet,
    summary: Option<&backlot_model::summary::StudioSummary>,
    palette: &Palette,
    shadow: bool,
) -> SheetRender {
    let desired = sheet_rect(band);
    if desired.width < 4 || desired.height < 4 {
        return SheetRender {
            frame: desired,
            body: Rect::default(),
        };
    }
    let Some(geom) = popup_frame::open_from_border_size(
        buf,
        band,
        desired.width,
        desired.height,
        4,
        4,
        palette.accent,
        PopupChrome::BorderJoined,
        palette,
        shadow,
    ) else {
        return SheetRender {
            frame: desired,
            body: Rect::default(),
        };
    };
    let frame = geom.border;
    let bg = overlay_bg(palette);

    // The title: `<studio> · task <id>` in the top border — the task
    // part only when the sheet opened on one.
    let title = match &sheet.task {
        Some(task) => format!(" {} · task {task} ", sheet.studio.id),
        None => format!(" {} ", sheet.studio.id),
    };
    let title_w = display_width(&title) as u16;
    if title_w < frame.width.saturating_sub(4) {
        put_text(
            buf,
            frame.x + 2,
            frame.y,
            title_w,
            &title,
            Style::default()
                .fg(palette.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }

    // The tab row under the top border: ` 1 Overview & Casts  2 …` —
    // the active tab keeps the accent, the rest dim.
    let tabs_y = frame.y + 1;
    let mut x = frame.x + 2;
    for (i, name) in SheetTab::NAMES.iter().enumerate() {
        let active = matches!(
            (i, sheet.tab),
            (0, SheetTab::Overview) | (1, SheetTab::Deliveries) | (2, SheetTab::Records)
        );
        let w = display_width(name) as u16 + 2;
        if x + w >= frame.x + frame.width.saturating_sub(1) {
            break;
        }
        put_text(
            buf,
            x,
            tabs_y,
            w,
            &format!(" {name} "),
            if active {
                Style::default()
                    .fg(glyphs::contrast_fg(palette))
                    .bg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.overlay0).bg(bg)
            },
        );
        x += w;
    }

    // Flush border-joined divider under tabs when vertical room allows.
    let edge = Style::default().fg(palette.accent).bg(bg);
    let div_rule = Style::default().fg(palette.elevation().border_muted).bg(bg);
    if frame.height >= 6 && frame.width > 2 {
        let div_y = frame.y + 2;
        put_text(buf, frame.x, div_y, 1, "├", edge);
        put_text(
            buf,
            frame.x + 1,
            div_y,
            frame.width - 2,
            &"─".repeat(usize::from(frame.width - 2)),
            div_rule,
        );
        put_text(buf, frame.right() - 1, div_y, 1, "┤", edge);
    }

    let raw_body_y = frame.y + 3;
    let footer_div_y = frame.bottom().saturating_sub(3);
    let has_footer_div = frame.height >= 12 && footer_div_y > raw_body_y + 1;
    if has_footer_div && frame.width > 2 {
        put_text(buf, frame.x, footer_div_y, 1, "├", edge);
        put_text(
            buf,
            frame.x + 1,
            footer_div_y,
            frame.width - 2,
            &"─".repeat(usize::from(frame.width - 2)),
            div_rule,
        );
        put_text(buf, frame.right() - 1, footer_div_y, 1, "┤", edge);
    }

    let raw_body_bottom = if has_footer_div {
        footer_div_y
    } else {
        frame.bottom().saturating_sub(2)
    };
    let raw_body_h = raw_body_bottom.saturating_sub(raw_body_y);
    let pad_bx: u16 = if frame.width >= 12 { 2 } else { 1 };
    let pad_by: u16 = u16::from(raw_body_h >= 6);
    let body = Rect::new(
        frame.x + 1 + pad_bx.saturating_sub(1),
        raw_body_y + pad_by,
        frame.width.saturating_sub(2 + 2 * pad_bx.saturating_sub(1)),
        raw_body_h.saturating_sub(2 * pad_by),
    );
    let lines = lines_for(sheet, summary);
    let scroll = sheet
        .scroll
        .min(lines.len().saturating_sub(body.height as usize));
    // An open report owns the body — the tab's lines must not paint
    // under it, or a shorter report line leaves the old text behind.
    for (at, line) in lines
        .iter()
        .skip(scroll)
        .take(if sheet.view.is_some() {
            0
        } else {
            body.height as usize
        })
        .enumerate()
    {
        let y = body.y + at as u16;
        let selected = sheet.view.is_none() && line.selectable && scroll + at == sheet.sel;
        if selected {
            buf.set_style(
                Rect::new(body.x, y, body.width, 1),
                glyphs::selection(palette),
            );
        }
        let text = truncate_end(&line.text, body.width.saturating_sub(1) as usize);
        put_text(
            buf,
            body.x + u16::from(selected),
            y,
            body.width.saturating_sub(u16::from(selected)),
            &text,
            if selected {
                glyphs::selection(palette)
            } else {
                row_style(line.row, palette)
            },
        );
        if selected {
            put_text(
                buf,
                body.x,
                y,
                1,
                glyphs::SELECTION_BAR,
                Style::default().fg(palette.accent).bg(palette.selection_bg),
            );
        }
    }

    let end_y = if sheet.tab == SheetTab::Overview
        && sheet.view.is_none()
        && summary.is_some_and(|s| s.state == "finished")
        && scroll + usize::from(body.height) >= lines.len()
    {
        let mut drawn = lines
            .len()
            .saturating_sub(scroll)
            .min(usize::from(body.height));
        while drawn > 0
            && lines
                .get(scroll + drawn - 1)
                .is_some_and(|l| l.text.trim().is_empty())
        {
            drawn -= 1;
        }
        if drawn + 1 < usize::from(body.height) {
            Some(body.y + drawn as u16 + 1)
        } else if pad_by > 0 {
            Some(body.bottom())
        } else if drawn < usize::from(body.height) {
            Some(body.y + drawn as u16)
        } else {
            None
        }
    } else {
        None
    };
    if let Some(y) = end_y {
        let gilt = palette.elevation().gilt_style(bg);
        let segs = intertitle(" T H E   E N D ", 33u16.min(body.width), Flank::Star);
        draw_intertitle(buf, body.x, y, body.width, &segs, |p| match p {
            Part::Rule | Part::Clasp => gilt,
            Part::Star => Style::default()
                .fg(palette.accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
            Part::Label => Style::default()
                .fg(palette.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        });
    }

    // The footer: scroll marks plus the keys the sheet owns.
    let footer_y = frame.y + frame.height - 2;
    let footer = if sheet.view.is_some() {
        " j/k scroll · esc back ".to_string()
    } else {
        let more = if scroll > 0 { "↑ " } else { "" };
        let below = if scroll + (body.height as usize) < lines.len() {
            "↓ "
        } else {
            ""
        };
        format!(" {more}{below}tab/1-3 tabs · j/k · ⏎ read · esc close ")
    };
    let fw = (display_width(&footer) as u16).min(frame.width.saturating_sub(2));
    put_text(
        buf,
        frame.x + frame.width - 1 - fw,
        footer_y,
        fw,
        &footer,
        Style::default().fg(palette.overlay0).bg(bg),
    );

    // Report reads (`sheet.view`) render through the unified top-level
    // Reader modal in `draw_overlays` (`R3-M2`).
    SheetRender {
        frame: geom.outer,
        body,
    }
}
