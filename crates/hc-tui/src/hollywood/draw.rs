//! The Call Sheet's renderer (S1): the docked Call Sheet band dropped
//! from the top over the whole width — title row, WAITING items,
//! HOLLYWOOD talk/live/compose, STUDIOS strip/frames, hints bar, and the
//! bottom perforation edge. Never reserves columns or rows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::draw_lower::{draw_footer, draw_lower};
use super::draw_sections::{BandCtx, draw_studios, draw_title_row, draw_waiting};
use super::plan;
use super::rows::{self, Row, Target};
use super::sheet::Status;
use super::view;
use super::{Confirm, Hollywood, LayerMode, LayerOpts, Section};
use crate::chrome::palette::Palette;
use crate::panel::data::{MachineSummary, spans_machines};

pub(crate) use super::draw_chips::{CHIP_GAP, paint_wait_chips};

/// What the mouse needs: the bar's whole rect, the title row, the
/// `STUDIOS N` focus label, the frames/chips and band rows with their
/// targets, the input line, the waiting list's scroll metrics and the
/// open view's map.
#[derive(Default)]
pub(crate) struct HollywoodRender {
    /// The band's rect — the click boundary (ends right below the marquee sill).
    pub area: Rect,
    /// The title row (expanded).
    pub title: Rect,
    /// The `STUDIOS N` header label (on perforation row or 1-row strip).
    pub studios_label: Rect,
    /// The studio frames or 1-row chip strip rect.
    pub body: Rect,
    /// True when STUDIOS currently draws as the 1-row chip strip.
    pub strip: bool,
    /// Number of studio chips drawn in the 1-row strip (excludes `[+ new]`).
    pub drawn_studios: usize,
    /// Hit rows with their targets — title, waiting/talk rows, frames/chips.
    pub rows: Vec<(Rect, Target)>,
    /// Action chips (`y`/`d`/`↵ read`): rect, target, key.
    pub chips: Vec<(Rect, Target, char)>,
    /// Full chip area per `Wait` row so clicks between chips do not activate row.
    pub chips_zones: Vec<(Rect, Target)>,
    /// The input line (expanded, an agent only).
    pub input: Rect,
    /// The waiting list's scroll metrics — the wheel and PageUp/Down.
    pub metrics: Option<ScrollMetrics>,
    /// The body's scrollbar track (stays empty; wheel/PageUp/Down still read it).
    pub scrollbar: Rect,
    /// Waiting rows visible at once.
    pub viewport_rows: usize,
    /// Frames or chips sit before the drawn window.
    pub more_left: bool,
    /// Frames or chips sit after it.
    pub more_right: bool,
    /// The open detail view's hit map.
    pub view: Option<view::ViewRender>,
    /// The input editor's cursor while the line is editing.
    pub cursor: Option<crate::chrome::help::CursorState>,
    /// Whether an animated spinner mark was drawn this frame.
    pub working_drawn: bool,
    /// Always `false` for the focus-dock layout (no external shadow row).
    #[cfg(test)]
    pub shadow_drawn: bool,
}

use crate::chrome::scroll::ScrollMetrics;

impl HollywoodRender {
    /// The point sits anywhere on the band.
    pub fn contains(&self, point: (u16, u16)) -> bool {
        self.area.contains(ratatui::layout::Position {
            x: point.0,
            y: point.1,
        })
    }
}

/// Render the bar into `frame`.
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
        LayerMode::Hidden | LayerMode::Collapsed => HollywoodRender::default(),
        LayerMode::Expanded => render_band(
            buf, frame, machines, local, status, hollywood, opts, palette,
        ),
    }
}

/// The expanded band in top-to-bottom zone order: Title -> WAITING ->
/// HOLLYWOOD -> STUDIOS -> Hints -> Marquee Sill.
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
    let (waits, band_rows) =
        rows::band_rows_with_echoes(machines, status, opts.agent, &hollywood.reply_echoes);
    let plan = plan::plan(frame, &waits, &band_rows, hollywood, opts);

    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));
    let confirming_wait = hollywood.confirm.as_ref().and_then(|c| match c {
        Confirm::Finish(key) => {
            let target = Target::Ready(key.clone());
            waits.iter().position(|w| w.target() == target)
        }
    });
    let kept_wait = selected_wait.or(confirming_wait);

    let scroll = hollywood
        .scroll
        .min(waits.len().saturating_sub(plan.shown.max(1)));
    let scroll = kept_wait.map_or(scroll, |sel| {
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
    let activity = if opts.agent {
        rows::hollywood_activity(machines, hollywood.pending.as_deref())
    } else {
        rows::HollywoodActivity::Idle
    };
    let mut hits = HollywoodRender {
        area,
        viewport_rows: plan.shown,
        working_drawn: activity.is_active(),
        ..HollywoodRender::default()
    };
    if area.is_empty() {
        return hits;
    }
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].reset();
        }
    }
    let elev = palette.elevation();
    buf.set_style(area, Style::default().bg(elev.surface));

    let studios_n = all_frames
        .iter()
        .filter(|f| matches!(f, rows::Frame::Studio(_)))
        .count();
    let sel_ref = (!opts.suppress_highlights)
        .then_some(hollywood.selected.as_ref())
        .flatten();
    let focus_zone = sel_ref.and_then(Section::focus_of);
    let studios_focused = focus_zone == Some(Section::Studios);
    let wait_focused = focus_zone == Some(Section::Waiting);
    let hollywood_focused = focus_zone == Some(Section::Hollywood);

    let band_wait_total = band_rows
        .iter()
        .filter(|r| matches!(r, Row::Wait(_)))
        .count();
    let scroll_range = (band_wait_total > plan.shown && plan.shown > 0)
        .then(|| (scroll + 1, (scroll + plan.shown).min(band_wait_total)));
    let now_ms = hollywood.now_ms.unwrap_or_else(crate::update_state::now_ms);

    let ctx = BandCtx {
        area,
        machines,
        local,
        local_label,
        multi,
        status,
        hollywood,
        opts,
        palette,
        all_frames: &all_frames,
        waits: &waits,
        band_rows: &band_rows,
        plan: &plan,
        scroll,
        scroll_range,
        activity,
        now_ms,
        studios_n,
        studios_focused,
        wait_focused,
        hollywood_focused,
        sel_ref,
    };

    let y = draw_title_row(buf, area.y, &ctx, &mut hits);
    let y = draw_waiting(buf, y, &ctx, &mut hits);
    let y = draw_lower(buf, y, &ctx, &mut hits);
    let y = draw_studios(buf, y, &ctx, &mut hits);
    draw_footer(buf, y, &ctx, &mut hits);
    hits
}
