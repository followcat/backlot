//! The Call Sheet's renderer (S1): the film-strip band dropped from
//! the top over the whole width — sprocket rows, the title row, one
//! frame per studio (or the chip strip), the headline, the waiting
//! items with their action chips and detail lines, the `tell
//! hollywood` line and the hint row. It never reserves a row or a
//! column: the pane layout is never recomputed for it.
//!
//! The frames live in `frames.rs`, the chip strip in `chips.rs`,
//! the waiting rows in `band.rs`, the section painters in
//! `draw_sections.rs` and `draw_lower.rs`, the wait-row action chips
//! in `draw_chips.rs`, the height ladder in `plan.rs`; this file owns
//! the hit map, the mode entry point and top-level band setup.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::draw_lower::{draw_footer, draw_lower};
use super::draw_sections::{BandCtx, draw_upper, draw_waiting};
use super::plan;
use super::rows::{self, Row, Target};
use super::sheet::Status;
use super::view;
use super::{Hollywood, LayerMode, LayerOpts};
use crate::chrome::palette::Palette;
use crate::panel::data::{MachineSummary, spans_machines};

pub(crate) use super::draw_chips::{CHIP_GAP, paint_wait_chips};

/// The title row's mark.
pub(crate) const MARK: &str = "✦  H O L L Y W O O D";
/// The title row's left margin.
pub(super) const MARGIN: u16 = 2;

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
    /// Full horizontal + vertical chip area per `Wait` row so clicks on
    /// the 1-cell gap between chips select the row without triggering
    /// row activation.
    pub chips_zones: Vec<(Rect, Target)>,
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
    /// Whether an animated spinner mark was drawn this frame.
    pub working_drawn: bool,
    /// Whether the 1-row external bottom drop shadow rendered below `area`.
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
        // The collapsed bar is the lot pill in the chrome's own row —
        // the floating strip is gone; Reader (`CardView`) renders as
        // a top-level dimmed modal in `draw_overlays` (`R3-M2`).
        LayerMode::Hidden | LayerMode::Collapsed => HollywoodRender::default(),
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
    // Reset every cell the band covers — pane borders and titles must
    // not show through its blanks.
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
    let studios_focused =
        sel_ref.is_some_and(|t| matches!(t, Target::Studio(_) | Target::NewStudio | Target::Title));
    let wait_focused = sel_ref.is_some_and(|t| {
        matches!(
            t,
            Target::Prompt(..)
                | Target::Role(..)
                | Target::Tell(..)
                | Target::Upgrade(..)
                | Target::Decision(..)
                | Target::Stage(..)
                | Target::Card(..)
                | Target::OutwardCard(..)
                | Target::Ready(..)
                | Target::Hollywood
        )
    });
    let hollywood_focused = sel_ref.is_some_and(|t| {
        matches!(
            t,
            Target::Headline
                | Target::Talk(_)
                | Target::HollywoodLive
                | Target::Input
                | Target::Setup
        )
    });

    let band_wait_total = band_rows
        .iter()
        .filter(|r| matches!(r, Row::Wait(_)))
        .count();
    let scroll_range = (band_wait_total > plan.shown && plan.shown > 0)
        .then(|| (scroll + 1, (scroll + plan.shown).min(band_wait_total)));
    let now_ms = hollywood.now_ms.unwrap_or_else(crate::update_state::now_ms);

    let ctx = BandCtx {
        frame,
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

    let y = draw_upper(buf, area.y, &ctx, &mut hits);
    let y = draw_waiting(buf, y, &ctx, &mut hits);
    let y = draw_lower(buf, y, &ctx, &mut hits);
    draw_footer(buf, y, &ctx, &mut hits);
    hits
}
