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
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs;

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
    match row {
        Row::Text => Style::default().fg(palette.text),
        Row::Head => Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
        Row::Dim => Style::default().fg(palette.overlay0),
        Row::Good => Style::default().fg(palette.green),
        Row::Bad => Style::default().fg(palette.red),
    }
}

/// Draw the whole sheet: dim the buffer (already done by the caller
/// like the other modals), the rounded frame, the studio/task title,
/// the three tabs, the scrolled content and the footer.
pub(crate) fn render(
    buf: &mut Buffer,
    band: Rect,
    sheet: &Sheet,
    summary: Option<&backlot_model::summary::StudioSummary>,
    palette: &Palette,
) -> SheetRender {
    let frame = sheet_rect(band);
    if frame.width < 4 || frame.height < 4 {
        return SheetRender {
            frame,
            body: Rect::default(),
        };
    }
    // The shared dialog frame — every modal paints the same
    // `panel_bg` box with an accent border (A6.2).
    let Some(_inner) = crate::chrome::dialogs::panel(buf, frame, palette.accent, palette.panel_bg)
    else {
        return SheetRender {
            frame,
            body: Rect::default(),
        };
    };

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
                .bg(palette.panel_bg)
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
                Style::default().fg(palette.overlay0)
            },
        );
        x += w;
    }

    let body = Rect::new(
        frame.x + 1,
        frame.y + 3,
        frame.width - 2,
        frame.height.saturating_sub(5),
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
                Style::default().fg(palette.accent),
            );
        }
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
        format!(" {more}{below}tab/1-3 tabs · j/k · o read · esc close ")
    };
    let fw = (display_width(&footer) as u16).min(frame.width.saturating_sub(2));
    put_text(
        buf,
        frame.x + frame.width - 1 - fw,
        footer_y,
        fw,
        &footer,
        Style::default().fg(palette.overlay0),
    );

    // The `o`-opened report covers the body: title line, then the
    // file's own text scrolled.
    if let Some(view) = &sheet.view {
        let head = format!(" {} ", view.title);
        put_text(
            buf,
            body.x,
            body.y,
            body.width,
            &truncate_end(&head, body.width as usize),
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        );
        let vscroll = view.scroll.min(view.lines.len().saturating_sub(1));
        for (at, l) in view
            .lines
            .iter()
            .skip(vscroll)
            .take(body.height.saturating_sub(1) as usize)
            .enumerate()
        {
            put_text(
                buf,
                body.x,
                body.y + 1 + at as u16,
                body.width,
                &truncate_end(l, body.width as usize),
                Style::default().fg(palette.text),
            );
        }
    }
    SheetRender { frame, body }
}
