//! The Call Sheet's renderer (S1): the film-strip band dropped from
//! the top over the whole width — sprocket rows, the title row, one
//! frame per studio (or the chip strip), the headline, the waiting
//! items with their action chips and detail lines, the `tell
//! hollywood` line and the hint row. It never reserves a row or a
//! column: the pane layout is never recomputed for it.
//!
//! The frames live in `frames.rs`, the chip strip in `chips.rs`,
//! the waiting rows in `band.rs`, the height ladder in `plan.rs`;
//! this file owns the band, the title row, the scroll window and
//! the hit map.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::plan;
use super::rows::{self, Row, Target};
use super::sheet::{Status, Wait};
use super::view;
use super::{Hollywood, LayerMode, LayerOpts, band, chips, frames, hints, items, title};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics};
use crate::chrome::text::display_width;
use crate::panel::data::{MachineSummary, spans_machines};

/// The title row's mark.
pub(crate) const MARK: &str = "✦  H O L L Y W O O D";
/// The title row's left margin.
const MARGIN: u16 = 2;

/// What the mouse needs: the bar's whole rect, the title row, the
/// frames and band rows with their targets, the input line, the
/// waiting list's scroll metrics and the open view's map.
#[derive(Default)]
pub(crate) struct HollywoodRender {
    /// The band's rect — the click boundary.
    pub area: Rect,
    /// The title row (expanded).
    pub title: Rect,
    /// The frames and band rows (expanded).
    pub body: Rect,
    /// Hit rows with their targets — the title, the frames/chips
    /// and the band rows alike (S1's mouse selects through them).
    pub rows: Vec<(Rect, Target)>,
    /// A waiting row's action chips (S5 §2): the chip's rect, its
    /// row's target and its key — a chip click selects its row and
    /// runs the key's action.
    pub chips: Vec<(Rect, Target, char)>,
    /// The input line (expanded, an agent only).
    pub input: Rect,
    /// The waiting list's scroll metrics — the wheel and PageUp/Down.
    pub metrics: Option<ScrollMetrics>,
    /// The body's scrollbar track. The film-strip design draws none,
    /// so this stays empty; the wheel and PageUp/Down still read it.
    pub scrollbar: Rect,
    /// Waiting rows visible at once.
    pub viewport_rows: usize,
    /// Frames sit before the drawn window.
    pub more_left: bool,
    /// Frames sit after it.
    pub more_right: bool,
    /// The open detail view's hit map.
    pub view: Option<view::ViewRender>,
    /// The input editor's cursor while the line is editing.
    pub cursor: Option<crate::chrome::help::CursorState>,
}

impl HollywoodRender {
    /// The point sits anywhere on the band.
    pub fn contains(&self, point: (u16, u16)) -> bool {
        self.area.contains(ratatui::layout::Position {
            x: point.0,
            y: point.1,
        })
    }
}

/// Render the bar into the frame. `machines` is every machine's last
/// good `backlot` document — the frames and the waiting items span
/// them all while the headline, the cards and the input line stay
/// this core's. `status` answers each machine's connectivity and
/// stage liveness; `local` is this core's display name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    frame: Rect,
    machines: &[MachineSummary],
    local: Option<&str>,
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    match hollywood.mode {
        LayerMode::Hidden => HollywoodRender::default(),
        // The collapsed bar is the lot pill in the chrome's own row —
        // the floating strip is gone, so a collapsed layer draws
        // nothing here.
        LayerMode::Collapsed => HollywoodRender::default(),
        LayerMode::Expanded => render_band(
            buf, frame, machines, local, status, hollywood, opts, palette,
        ),
    }
}

/// The expanded band: the plan, the frames or chips, the headline,
/// the waiting rows' window, the tell line, the hints and the two
/// sprocket rows, all over `surface_dim`.
#[allow(clippy::too_many_arguments)]
fn render_band(
    buf: &mut Buffer,
    frame: Rect,
    machines: &[MachineSummary],
    local: Option<&str>,
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> HollywoodRender {
    if frame.width == 0 || frame.height == 0 {
        return HollywoodRender::default();
    }
    let multi = spans_machines(machines);
    let local_label = local.unwrap_or("Local");
    let all_frames = rows::frames(machines);
    let (waits, band_rows) = rows::band_rows(machines, status, opts.agent);
    let plan = plan::plan(frame, &waits, &band_rows, hollywood, opts);
    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));

    // The scroll window: `scroll` picks the first waiting row drawn;
    // `shown` how many. Keep the selected row inside the window.
    let scroll = hollywood
        .scroll
        .min(waits.len().saturating_sub(plan.shown.max(1)));
    let scroll = selected_wait.map_or(scroll, |sel| {
        if sel < scroll {
            sel
        } else if sel >= scroll + plan.shown {
            (sel + 1).saturating_sub(plan.shown)
        } else {
            scroll
        }
    });

    let area = Rect::new(
        frame.x,
        frame.y,
        frame.width,
        plan.height().min(frame.height),
    );
    let mut hits = HollywoodRender {
        area,
        viewport_rows: plan.shown,
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
        title::draw_sprocket(buf, area, y, '▄', palette);
        y += 1;
    }
    if plan.title {
        title::draw_title(
            buf, area, y, machines, &waits, opts.agent, palette, &mut hits,
        );
        y += 1;
    }
    if plan.blank_1 {
        y += 1;
    }
    if plan.frames_h == frames::FRAME_H {
        let rect = Rect::new(area.x, y, area.width, plan.frames_h);
        let drawn = frames::render(
            buf,
            rect,
            &all_frames,
            frames::FramesView {
                selected: hollywood.selected.as_ref(),
                scroll: hollywood.frame_scroll,
                mobile: opts.mobile,
                local,
                multi,
                status,
            },
            palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += drawn.height;
    } else if plan.frames_h == 1 {
        let rect = Rect::new(area.x, y, area.width, 1);
        let drawn = chips::render(
            buf,
            rect,
            &all_frames,
            status,
            hollywood.selected.as_ref(),
            hollywood.frame_scroll,
            local,
            multi,
            palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += 1;
    }
    if plan.blank_2 {
        y += 1;
    }
    // The band rows: headline, then the scroll window of waiting
    // items (each with its detail when kept), then the tail — the
    // calm row or the input/setup line.
    let mut wait_seen = 0usize;
    for row in &band_rows {
        match row {
            Row::Headline(headline) if plan.headline => {
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    &Row::Headline(headline),
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    palette,
                    &mut hits,
                );
                y += 1;
            }
            Row::Wait(i) => {
                let index = *i;
                let in_window = wait_seen >= scroll && wait_seen < scroll + plan.shown;
                wait_seen += 1;
                if !in_window {
                    continue;
                }
                let detail = plan.details.get(index).copied().unwrap_or(false);
                let h = 1 + u16::from(detail);
                let rect = Rect::new(area.x, y, area.width, h);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    palette,
                    &mut hits,
                );
                y += h;
            }
            Row::Calm { .. } | Row::Input | Row::Setup => {
                // The scroll marker lives on this row's right edge
                // when items hid — `2/6` of the sheet showing.
                let rect = Rect::new(area.x, y, area.width, 1);
                band::draw_row(
                    buf,
                    rect,
                    row,
                    &waits,
                    hollywood,
                    local_label,
                    multi,
                    opts.mobile,
                    palette,
                    &mut hits,
                );
                if waits.len() > plan.shown {
                    let mark = format!(
                        "{}/{waits_len}",
                        scroll + plan.shown.min(waits.len()),
                        waits_len = waits.len()
                    );
                    let w = mark.chars().count() as u16;
                    put_text(
                        buf,
                        (rect.x + rect.width).saturating_sub(MARGIN + w),
                        rect.y,
                        w,
                        &mark,
                        Style::default()
                            .fg(palette.overlay0)
                            .bg(palette.surface_dim),
                    );
                }
                y += 1;
            }
            _ => {}
        }
    }
    hits.metrics = Some(list_scroll_metrics(
        &vec![1; waits.len()],
        &[],
        plan.shown.max(1) as u16,
        scroll,
    ));
    if plan.hints {
        let usable = (area.width as usize).saturating_sub((MARGIN * 2) as usize);
        let tier = hints::for_row(hollywood.selected.as_ref(), &waits, usable);
        let w = tier.chars().count() as u16;
        let x = (area.x + area.width.saturating_sub(MARGIN))
            .saturating_sub(w)
            .max(area.x);
        put_text(
            buf,
            x,
            y,
            w.min(area.width),
            &tier,
            Style::default()
                .fg(palette.overlay0)
                .bg(palette.surface_dim),
        );
        y += 1;
    }
    if plan.bottom_sprocket {
        title::draw_sprocket(buf, area, y, '▀', palette);
    }
    if let Some(view) = hollywood.view.as_ref() {
        hits.view = Some(view::render_view(
            buf,
            area,
            view,
            hollywood.pending.as_deref(),
            palette,
        ));
    }
    hits
}

/// A waiting row's action chips (S5 §2), painted right-aligned from
/// `x` — the primary chip lit `accent` on the selected row — and
/// recorded as click targets: `↵` maps to the row's Enter, a letter
/// to the row's own key.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_wait_chips(
    buf: &mut Buffer,
    y: u16,
    mut x: u16,
    wait: &Wait<'_>,
    chips: &[items::Chip],
    compact: bool,
    selected: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    for chip in chips {
        let text = items::chip_text(chip, compact);
        let w = display_width(&text) as u16;
        let style = if selected && chip.primary {
            Style::default()
                .fg(crate::studio::glyphs::contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.subtext0).bg(palette.surface0)
        };
        put_text(buf, x, y, w, &text, style);
        let key = if chip.key == "↵" {
            '\r'
        } else {
            chip.key.chars().next().unwrap_or('\r')
        };
        hits.chips.push((Rect::new(x, y, w, 1), wait.target(), key));
        x += w;
    }
}
