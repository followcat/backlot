//! The bar's renderer (U7): the collapsed strip at the top row's
//! right end, or the film-strip band dropped from the top over the
//! whole width — sprocket rows, the title row, one frame per studio
//! and the new-studio frame, then the headline, the cards and the
//! input line. It never reserves a row or a column: the pane layout
//! is never recomputed for it.
//!
//! The frames live in `frames.rs`, the band rows in `band.rs`; this
//! file owns the strip, the title row, the height ladder and the
//! hit map.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::plan::{self, Plan};
use super::rows::{self, Row, Target};
use super::view;
use super::{Hollywood, LayerMode, LayerOpts, band, frames, strip};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics};

/// The expanded band's height cap while floating: half the frame.
const EXPANDED_FRAC: u16 = 2;
/// The title row's mark.
const MARK: &str = "✦  H O L L Y W O O D";
/// The title row's left margin.
const MARGIN: u16 = 2;

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

/// What the mouse needs: the bar's whole rect, the title row, the
/// frames and band rows with their targets, each card's buttons, the
/// input line, the cards' scroll metrics and the open view's map.
#[derive(Default)]
pub(crate) struct HollywoodRender {
    /// The strip's or the band's rect — the click boundary.
    pub area: Rect,
    /// The title row (expanded).
    pub title: Rect,
    /// The frames and band rows (expanded).
    pub body: Rect,
    /// Hit rows with their targets — the title, the frames and the
    /// band rows alike.
    pub rows: Vec<(Rect, Target)>,
    /// Per-card Apply/Cancel/Show rects.
    pub buttons: Vec<(Rect, CardBtn)>,
    /// The input line (expanded, an agent only).
    pub input: Rect,
    /// The card list's scroll metrics — the wheel and PageUp/Down.
    pub metrics: Option<ScrollMetrics>,
    /// The body's scrollbar track. The film-strip design draws none,
    /// so this stays empty; the wheel, the drag math and PageUp/Down
    /// still read it.
    pub scrollbar: Rect,
    /// Card rows visible at once.
    pub viewport_rows: usize,
    /// Frames sit before the drawn window.
    pub more_left: bool,
    /// Frames sit after it.
    pub more_right: bool,
    /// The open "Show me" view's hit map.
    pub view: Option<view::ViewRender>,
    /// The input editor's cursor while the line is editing.
    pub cursor: Option<crate::chrome::help::CursorState>,
}

impl HollywoodRender {
    /// The point sits on the bar — a strip click or anywhere in the
    /// band.
    pub fn contains(&self, point: (u16, u16)) -> bool {
        self.area.contains(ratatui::layout::Position {
            x: point.0,
            y: point.1,
        })
    }
}

/// Render the bar into the frame. `summary` is this client's last good
/// `backlot` document — `None` keeps the frames and the band on their
/// empty reads.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    match hollywood.mode {
        LayerMode::Hidden => HollywoodRender::default(),
        LayerMode::Collapsed => strip::render(buf, frame, summary, opts.agent, palette),
        LayerMode::Expanded => {
            render_band(buf, frame, summary, stage_running, hollywood, opts, palette)
        }
    }
}

/// The expanded band: the plan, the frames, the band rows, the hints
/// and the two sprocket rows, all over `surface_dim`.
fn render_band(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    if frame.width == 0 || frame.height == 0 {
        return HollywoodRender::default();
    }
    let all_frames = rows::frames(summary);
    let band_rows = rows::band_rows(summary, stage_running, opts.agent);
    let cap = if opts.mobile {
        frame.height
    } else {
        (frame.height / EXPANDED_FRAC).max(3).min(frame.height)
    };
    let (per_row, _) = frames::layout(frame.width, opts.mobile);
    let frame_rows = all_frames.len().div_ceil(per_row).min(u16::MAX as usize) as u16;
    let mut plan = Plan {
        top_sprocket: true,
        title: true,
        blank_1: true,
        frames_h: if opts.mobile {
            frame_rows * frames::FRAME_H
        } else {
            frames::FRAME_H
        },
        blank_2: true,
        band: (0..band_rows.len()).collect(),
        hints: true,
        bottom_sprocket: true,
    };
    plan::fit(&mut plan, &band_rows, cap, opts.mobile);

    // The cards' window: the plan keeps this many card rows, the
    // scroll offset picks which.
    let cards = band_rows
        .iter()
        .filter(|row| matches!(row, Row::Card(_)))
        .count();
    let card_slots = plan
        .band
        .iter()
        .filter(|&&i| matches!(band_rows[i], Row::Card(_)))
        .count();
    let start = hollywood.scroll.min(cards.saturating_sub(card_slots));
    let mut seen = 0;
    plan.band.retain(|&i| match band_rows[i] {
        Row::Card(_) => {
            let keep = seen >= start && seen < start + card_slots;
            seen += 1;
            keep
        }
        _ => true,
    });

    let area = Rect::new(
        frame.x,
        frame.y,
        frame.width,
        plan.height().min(frame.height),
    );
    let mut hits = HollywoodRender {
        area,
        ..HollywoodRender::default()
    };
    if area.is_empty() {
        return hits;
    }
    // Reset every cell the band covers — pane borders and titles must
    // not show through its blanks.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].reset();
        }
    }
    buf.set_style(area, Style::default().bg(palette.surface_dim));

    let mut y = area.y;
    if plan.top_sprocket {
        draw_sprocket(buf, area, y, '▄', palette);
        y += 1;
    }
    if plan.title {
        draw_title(buf, area, y, summary, opts.agent, palette, &mut hits);
        y += 1;
    }
    if plan.blank_1 {
        y += 1;
    }
    if plan.frames_h > 0 {
        let rect = Rect::new(area.x, y, area.width, plan.frames_h);
        let drawn = frames::render(
            buf,
            rect,
            &all_frames,
            frames::FramesView {
                selected: hollywood.selected.as_ref(),
                scroll: hollywood.frame_scroll,
                mobile: opts.mobile,
            },
            palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += drawn.height;
    }
    if plan.blank_2 {
        y += 1;
    }
    let mut shown = 0;
    for &index in &plan.band {
        let rect = Rect::new(area.x, y, area.width, 1);
        band::draw_row(buf, rect, &band_rows[index], hollywood, palette, &mut hits);
        if matches!(band_rows[index], Row::Card(_)) {
            shown += 1;
        }
        y += 1;
    }
    hits.metrics = Some(list_scroll_metrics(
        &vec![1; cards],
        &[],
        shown.max(1),
        start,
    ));
    if plan.hints {
        band::draw_hints(
            buf,
            Rect::new(area.x, y, area.width, 1),
            opts.agent,
            palette,
        );
        y += 1;
    }
    if plan.bottom_sprocket {
        draw_sprocket(buf, area, y, '▀', palette);
    }
    if let Some(view) = hollywood.view.as_ref() {
        hits.view = Some(view::render_view(buf, frame, view, palette));
    }
    hits
}

/// One sprocket row: `▄▄`/`▀▀` every four columns on `sidebar_bg`,
/// matching the opening title card.
fn draw_sprocket(buf: &mut Buffer, area: Rect, y: u16, glyph: char, palette: &Palette) {
    buf.set_style(
        Rect::new(area.x, y, area.width, 1),
        Style::default().bg(palette.sidebar_bg),
    );
    let style = Style::default().fg(palette.surface1).bg(palette.sidebar_bg);
    let hole = format!("{glyph}{glyph}");
    let mut x = area.x + 1;
    while x + 1 < area.x + area.width.saturating_sub(1) {
        put_text(buf, x, y, 2, &hole, style);
        x += 4;
    }
}

/// The display width of `text`, in cells — a floor for the title row's
/// room sums.
fn width_of(text: &str) -> u16 {
    crate::chrome::text::display_width(text) as u16
}

/// The title row: the mark, the agent's square and role, and the
/// right-aligned studio count with what waits on you.
fn draw_title(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    summary: Option<&Summary>,
    agent: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let row = Rect::new(area.x, y, area.width, 1);
    hits.title = row;
    hits.rows.push((row, Target::Title));
    let base = Style::default().bg(palette.surface_dim);
    let mark_w = MARK.chars().count() as u16;
    put_text(
        buf,
        area.x + MARGIN,
        y,
        mark_w,
        MARK,
        base.fg(palette.accent).add_modifier(Modifier::BOLD),
    );
    let x = area.x + MARGIN + mark_w + 3;

    let studios = summary.map(|s| s.studios.len()).unwrap_or(0);
    let waiting: u32 = summary
        .map(|s| s.studios.iter().map(|studio| studio.waiting).sum())
        .unwrap_or(0);
    let count = format!("{studios} studio{}", if studios == 1 { "" } else { "s" });
    let wait = if waiting > 0 {
        format!("  ·  {waiting} waiting on you")
    } else {
        String::new()
    };
    let state = summary
        .and_then(|s| s.hollywood.as_ref())
        .map(|h| h.state.as_str());
    // The agent's words: the full line when it fits, the state alone
    // when it does not. If even that is squeezed, the counts step
    // aside — the state is what the row must say.
    let (agent_line, short_line) = if agent {
        let state = state.unwrap_or("down");
        let details = summary
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| {
                [h.runner.as_deref(), h.model.as_deref(), h.effort.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        if details.is_empty() {
            (state.to_string(), String::new())
        } else {
            (format!("{state} · {details}"), state.to_string())
        }
    } else {
        ("no agent".to_string(), String::new())
    };
    let mut right_w = count.chars().count() as u16 + wait.chars().count() as u16;
    let mut right_x = (area.x + area.width).saturating_sub(MARGIN + right_w);
    let mut room = right_x.saturating_sub(x + 2);
    // The longest line that fits; if none does, the counts step aside
    // for it — the state is what the row must say.
    let pick = |room: u16| {
        if width_of(&agent_line) <= room {
            agent_line.clone()
        } else if !short_line.is_empty() && width_of(&short_line) <= room {
            short_line.clone()
        } else {
            String::new()
        }
    };
    let mut text = pick(room);
    if text.is_empty() && right_w > 0 {
        right_w = 0;
        right_x = area.x + area.width;
        room = right_x.saturating_sub(x + 2);
        text = pick(room);
    }
    if agent {
        let colour = strip::state_color(state.unwrap_or("down"), palette);
        put_text(buf, x, y, 1, "■", base.fg(colour));
    } else {
        put_text(buf, x, y, 1, "■", base.fg(palette.overlay0));
    }
    if !text.is_empty() {
        put_text(
            buf,
            x + 2,
            y,
            room,
            &text,
            base.fg(if agent {
                palette.overlay1
            } else {
                palette.overlay0
            }),
        );
    }
    if right_w > 0 && right_x > x {
        put_text(
            buf,
            right_x,
            y,
            count.chars().count() as u16,
            &count,
            base.fg(palette.overlay0),
        );
        if !wait.is_empty() {
            put_text(
                buf,
                right_x + count.chars().count() as u16,
                y,
                wait.chars().count() as u16,
                &wait,
                base.fg(palette.peach),
            );
        }
    }
}
