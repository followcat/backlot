//! The layer's renderer (H4): the collapsed strip at the top row's
//! right end, or the expanded panel dropped from the top — a
//! full-screen view at narrow widths, like the studio panel's.
//! Everything draws over the frame after the panes: nothing here
//! reserves a row or a column. Pure drawing — it reads `Hollywood`
//! and the document, writes cells and returns the hit map.

use backlot_model::summary::{StudioSummary, Summary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::view::{self, CardView};
use super::{Hollywood, LayerMode, Row, Target, rows, strip_text};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::truncate_end;

/// The collapsed strip's width cap: a third of the frame.
const STRIP_FRAC: u16 = 3;
/// The expanded panel's height cap while floating: half the frame.
const EXPANDED_FRAC: u16 = 2;

/// A card row's three action targets (Apply / Cancel / Show me).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardBtn {
    pub card: String,
    pub kind: BtnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BtnKind {
    Apply,
    Cancel,
    Show,
}

/// What the mouse needs: the strip's or the panel's whole rect, the
/// clickable rows with their targets, each card's buttons, the input
/// line, the scrollbar and the open view's map.
#[derive(Default)]
pub(crate) struct HollywoodRender {
    /// The strip's or the expanded panel's rect — the click boundary.
    pub area: Rect,
    /// The title row (expanded).
    pub title: Rect,
    /// The scrollable body between title and input (expanded).
    pub body: Rect,
    /// Hit rows with their targets — strip and panel rows alike.
    pub rows: Vec<(Rect, Target)>,
    /// Per-card Apply/Cancel/Show rects.
    pub buttons: Vec<(Rect, CardBtn)>,
    /// The input line (expanded).
    pub input: Rect,
    /// The body's scrollbar track when it overflows.
    pub scrollbar: Rect,
    pub metrics: Option<ScrollMetrics>,
    /// Rows visible per page (PageUp/PageDown step).
    pub viewport_rows: usize,
    /// The open "Show me" view's hit map.
    pub view: Option<view::ViewRender>,
    /// The input editor's cursor while the line is editing.
    pub cursor: Option<crate::chrome::help::CursorState>,
}

impl HollywoodRender {
    /// The point sits on the layer — a strip click or anywhere in the
    /// expanded panel.
    pub fn contains(&self, point: (u16, u16)) -> bool {
        self.area.contains(ratatui::layout::Position {
            x: point.0,
            y: point.1,
        })
    }
}

/// Render the layer into the frame. `summary` is this client's last
/// good `backlot` document — `None` keeps the strip and rows on their
/// down/empty reads. `mobile` is the narrow-width full-screen cut.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    hollywood: &Hollywood,
    mobile: bool,
    palette: &Palette,
) -> HollywoodRender {
    match hollywood.mode {
        LayerMode::Hidden => HollywoodRender::default(),
        LayerMode::Collapsed => render_strip(buf, frame, summary, palette),
        LayerMode::Expanded => render_expanded(
            buf,
            frame,
            summary,
            stage_running,
            hollywood,
            mobile,
            palette,
        ),
    }
}

/// `✦ hollywood live · 2 live 1 fin · 1 card · headline…` — one row
/// at the right end of the top row, sized to its content, at most a
/// third of the width. A click on it expands the layer.
fn render_strip(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    palette: &Palette,
) -> HollywoodRender {
    if frame.width == 0 || frame.height == 0 {
        return HollywoodRender::default();
    }
    let text = strip_text(summary);
    let cap = (frame.width / STRIP_FRAC).max(1);
    let text = truncate_end(&text, cap as usize);
    let width = (text.chars().count() as u16).min(cap).max(1);
    let strip = Rect::new(frame.x + frame.width - width, frame.y, width, 1);
    buf.set_style(strip, Style::default().bg(palette.panel_bg));
    put_text(
        buf,
        strip.x,
        strip.y,
        width,
        &text,
        Style::default().fg(palette.accent),
    );
    let mut hits = HollywoodRender {
        area: strip,
        ..HollywoodRender::default()
    };
    hits.rows.push((strip, Target::Title));
    hits
}

/// A state word's color: live green, finished/done teal, lost or
/// closed red, down dim — mapped through the palette, never
/// hardcoded.
fn state_color(state: &str, palette: &Palette) -> ratatui::style::Color {
    match state {
        "live" => palette.green,
        "finished" => palette.teal,
        "lost" | "closed" => palette.red,
        _ => palette.overlay0,
    }
}

/// The expanded panel: title, a scrollable body, the input line and
/// a bottom edge when it floats. At `mobile` widths it is the whole
/// frame and drops the edge row.
fn render_expanded(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    hollywood: &Hollywood,
    mobile: bool,
    palette: &Palette,
) -> HollywoodRender {
    let all = rows(summary, stage_running);
    // Fixed: title (1) + input (1). Floating adds the edge row (1).
    let edge = u16::from(!mobile);
    let body_len = all.len().saturating_sub(2) as u16;
    let want = 2 + body_len + edge;
    let height = if mobile {
        frame.height
    } else {
        want.clamp(3, (frame.height / EXPANDED_FRAC).max(3).min(frame.height))
    };
    let area = Rect::new(frame.x, frame.y, frame.width, height);
    let mut hits = HollywoodRender {
        area,
        ..HollywoodRender::default()
    };
    if area.is_empty() {
        return hits;
    }
    // Reset every cell the layer covers — pane borders and titles
    // must not show through its blanks.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].reset();
        }
    }
    buf.set_style(area, Style::default().bg(palette.panel_bg));

    // The bottom edge: `───` over the full width while floating.
    if !mobile {
        let y = area.y + area.height - 1;
        put_text(
            buf,
            area.x,
            y,
            area.width,
            &"─".repeat(usize::from(area.width)),
            Style::default().fg(palette.surface_dim),
        );
    }

    let interior = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(edge));
    if interior.height < 2 {
        return hits;
    }

    let dim = Style::default().fg(palette.overlay0);
    // The title row — `✦ hollywood <state> · <runner>/<model>/<effort>`.
    let title = Rect::new(interior.x, interior.y, interior.width, 1);
    hits.title = title;
    hits.rows.push((title, Target::Title));
    let mut title_text = "✦ hollywood".to_string();
    let mut title_style = Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD);
    if let Some(hollywood_summary) = summary.and_then(|s| s.hollywood.as_ref()) {
        let state = &hollywood_summary.state;
        title_text.push_str(&format!(" {state}"));
        let runner = [
            hollywood_summary.runner.as_deref(),
            hollywood_summary.model.as_deref(),
            hollywood_summary.effort.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("/");
        if !runner.is_empty() {
            title_text.push_str(&format!(" · {runner}"));
        }
        title_style = title_style.fg(state_color(state, palette));
    } else {
        title_text.push_str(" down");
        title_style = title_style.fg(palette.overlay0);
    }
    put_text(
        buf,
        title.x,
        title.y,
        title.width,
        &truncate_end(&title_text, title.width as usize),
        title_style,
    );

    // The input line is the last interior row — always in place,
    // editing or not.
    let input = Rect::new(
        interior.x,
        interior.y + interior.height - 1,
        interior.width,
        1,
    );
    hits.input = input;
    hits.rows.push((input, Target::Input));
    draw_input(buf, input, hollywood, dim, palette, &mut hits);

    // The scrollable body between them.
    let body = Rect::new(
        interior.x,
        interior.y + 1,
        interior.width,
        interior.height.saturating_sub(2),
    );
    if body.height == 0 {
        finish(buf, frame, hollywood, palette, &mut hits);
        return hits;
    }
    hits.body = body;
    let heights: Vec<u16> = all
        .iter()
        .skip(1)
        .take(body_len as usize)
        .map(|_| 1)
        .collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, hollywood.scroll);
    hits.metrics = Some(metrics);
    hits.viewport_rows = metrics.viewport_rows;
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = body.width - u16::from(show_bar && body.width > 1);
    let selected = hollywood.selected.as_ref();

    for (index, row) in all.iter().enumerate().skip(1 + start) {
        if matches!(row, Row::Input) {
            break;
        }
        let y = body.y + (index - 1 - start) as u16;
        if y >= body.y + body.height {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, 1);
        let target = row.target();
        if selected == Some(&target)
            && matches!(row, Row::Studio(_) | Row::Card(_) | Row::Headline(_))
        {
            buf.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        match row {
            Row::Title | Row::Input => {}
            Row::StageDown => put_text(buf, rect.x, rect.y, rect.width, " stage not running", dim),
            Row::Empty => put_text(buf, rect.x, rect.y, rect.width, " no studios", dim),
            Row::Studio(studio) => draw_studio(buf, rect, studio, palette),
            Row::Headline(headline) => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end(&format!(" ✧ {headline}"), rect.width as usize),
                    Style::default().fg(palette.subtext0),
                );
            }
            Row::Card(card) => draw_card(buf, rect, card, palette, &mut hits),
        }
        hits.rows.push((rect, target));
    }
    if show_bar {
        hits.scrollbar = Rect::new(body.x + body.width - 1, body.y, 1, body.height);
        render_list_scrollbar(buf, hits.scrollbar, metrics, palette);
    }
    finish(buf, frame, hollywood, palette, &mut hits);
    hits
}

/// The input line: `› <text>` — the dim `tell hollywood …` hint while
/// empty and not editing; the editor's own cursor while editing.
fn draw_input(
    buf: &mut Buffer,
    input: Rect,
    hollywood: &Hollywood,
    dim: Style,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    if input.width < 3 {
        return;
    }
    put_text(
        buf,
        input.x,
        input.y,
        2,
        "› ",
        Style::default().fg(palette.accent),
    );
    let field = Rect::new(input.x + 2, input.y, input.width - 2, 1);
    if hollywood.editing {
        hits.cursor = crate::chrome::text_editor::render(
            buf,
            field,
            &hollywood.input,
            Style::default().fg(palette.text),
        );
    } else if hollywood.input.as_str().is_empty() {
        put_text(
            buf,
            field.x,
            field.y,
            field.width,
            "i — tell hollywood …",
            dim,
        );
    } else {
        put_text(
            buf,
            field.x,
            field.y,
            field.width,
            &truncate_end(hollywood.input.as_str(), field.width as usize),
            Style::default().fg(palette.text),
        );
    }
}

/// `<id> · <state> · !<waiting> · <next>` — name, state, attention
/// count and what the studio is doing.
fn draw_studio(buf: &mut Buffer, rect: Rect, studio: &StudioSummary, palette: &Palette) {
    let mut text = format!(" {} · {}", studio.id, studio.state);
    if studio.waiting > 0 {
        text.push_str(&format!(" · !{}", studio.waiting));
    }
    if let Some(next) = studio.next.as_deref() {
        text.push_str(&format!(" · {next}"));
    } else if studio.error.is_some() {
        text.push_str(" · ! ledger");
    }
    put_text(
        buf,
        rect.x,
        rect.y,
        rect.width,
        &truncate_end(&text, rect.width as usize),
        Style::default().fg(state_color(&studio.state, palette)),
    );
}

/// `c<n> · <studio> · <action> — <why>` plus the right-aligned
/// `apply cancel show` targets — each its own hit rect.
fn draw_card(
    buf: &mut Buffer,
    rect: Rect,
    card: &backlot_model::summary::CardSummary,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    const BUTTONS: [(&str, BtnKind); 3] = [
        ("apply", BtnKind::Apply),
        ("cancel", BtnKind::Cancel),
        ("show", BtnKind::Show),
    ];
    // Buttons hug the right edge: ` apply cancel show ` — 4+6+4 chars
    // plus their separators.
    let buttons_width: u16 = BUTTONS
        .iter()
        .map(|(label, _)| label.len() as u16 + 1)
        .sum();
    let text_width = rect.width.saturating_sub(buttons_width);
    let text = format!(
        " {} · {} · {} — {}",
        card.id, card.studio, card.action, card.why
    );
    put_text(
        buf,
        rect.x,
        rect.y,
        text_width,
        &truncate_end(&text, text_width as usize),
        Style::default().fg(palette.text),
    );
    let mut x = rect.x + text_width;
    for (label, kind) in BUTTONS {
        let w = label.len() as u16 + 1;
        if x + w > rect.x + rect.width {
            break;
        }
        let btn = Rect::new(x + 1, rect.y, w - 1, 1);
        put_text(
            buf,
            btn.x,
            btn.y,
            btn.width,
            label,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::UNDERLINED),
        );
        hits.buttons.push((
            btn,
            CardBtn {
                card: card.id.clone(),
                kind,
            },
        ));
        x += w;
    }
}

/// The open "Show me" view draws last — over the layer itself.
fn finish(
    buf: &mut Buffer,
    frame: Rect,
    hollywood: &Hollywood,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    if let Some(view) = hollywood.view.as_ref() {
        hits.view = Some(view::render_view(buf, frame, view, palette));
    }
}

#[allow(dead_code)]
fn _assert_cardview(_: &CardView) {}
