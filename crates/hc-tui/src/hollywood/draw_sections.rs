//! Title, WAITING, and STUDIOS section painters for the docked Call
//! Sheet band (`Title -> WAITING -> HOLLYWOOD -> STUDIOS -> Footer`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::band::RowExtras;
use super::draw::HollywoodRender;
use super::plan::Plan;
use super::rows::{Frame, HollywoodActivity, Row, Target};
use super::sheet::{Status, Wait};
use super::{Hollywood, LayerOpts, band, chips, frames, title, zone};
use crate::chrome::palette::Palette;
use crate::panel::data::MachineSummary;

/// Shared inputs for the expanded band's section painters.
pub(super) struct BandCtx<'a> {
    pub area: Rect,
    pub machines: &'a [MachineSummary],
    pub local: Option<&'a str>,
    pub local_label: &'a str,
    pub multi: bool,
    pub status: &'a Status<'a>,
    pub hollywood: &'a Hollywood,
    pub opts: LayerOpts,
    pub palette: &'a Palette,
    pub all_frames: &'a [Frame<'a>],
    pub waits: &'a [Wait<'a>],
    pub band_rows: &'a [Row<'a>],
    pub plan: &'a Plan,
    pub scroll: usize,
    pub scroll_range: Option<(usize, usize)>,
    pub activity: HollywoodActivity,
    pub now_ms: u64,
    pub studios_n: usize,
    pub studios_focused: bool,
    pub wait_focused: bool,
    pub hollywood_focused: bool,
    pub sel_ref: Option<&'a Target>,
}

/// Title row (`✦  H O L L Y W O O D` + right bucket counts + scroll window).
pub(super) fn draw_title_row(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    if ctx.plan.title {
        title::draw_title(
            buf,
            ctx.area,
            y,
            ctx.machines,
            ctx.waits,
            ctx.opts.agent,
            ctx.activity,
            ctx.hollywood.spin,
            ctx.scroll_range,
            ctx.palette,
            hits,
        );
        y += 1;
    }
    y
}

/// Top zone: visible waiting items (`1` line unfocused, expanded on the
/// selected row with col-0 `▌` down the zone when focused).
pub(super) fn draw_waiting(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    let mut wait_seen = 0usize;
    let mut first_drawn = true;
    for row in ctx.band_rows {
        if let Row::Wait(i) = row {
            let index = *i;
            let in_window = wait_seen >= ctx.scroll && wait_seen < ctx.scroll + ctx.plan.shown;
            wait_seen += 1;
            if !in_window {
                continue;
            }
            let detail = ctx.plan.details.get(index).copied().unwrap_or(false);
            let h = 1 + if detail {
                1 + ctx.plan.detail_extra.get(index).copied().unwrap_or(0)
            } else {
                0
            };
            let wait_scroll_badge = if first_drawn && !ctx.plan.title {
                ctx.scroll_range
                    .map(|(top, bot)| (top, bot, ctx.waits.len()))
            } else {
                None
            };
            first_drawn = false;
            let rect = Rect::new(ctx.area.x, y, ctx.area.width, h);
            band::draw_row(
                buf,
                rect,
                row,
                ctx.waits,
                ctx.hollywood,
                ctx.local_label,
                ctx.multi,
                ctx.opts.mobile,
                ctx.opts.suppress_highlights,
                ctx.activity,
                ctx.now_ms,
                RowExtras {
                    wait_focused: ctx.wait_focused,
                    wait_scroll_badge,
                    talk_earlier: 0,
                    ..RowExtras::default()
                },
                ctx.palette,
                hits,
            );
            y += h;
        }
    }
    y
}

/// Bottom zone: either centered perforation header + `7`/`5`/`3`-row studio
/// frames (when STUDIOS is focused and room fits) or the 1-row chip strip.
pub(super) fn draw_studios(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    if ctx.plan.studios_perf {
        zone::draw_studios_perforation(
            buf,
            ctx.area,
            y,
            ctx.studios_n,
            ctx.studios_focused,
            ctx.palette,
            hits,
        );
        y += 1;
    }
    if ctx.plan.frames_h >= frames::FRAME_H_MIN {
        let rect = Rect::new(ctx.area.x, y, ctx.area.width, ctx.plan.frames_h);
        let drawn = frames::render(
            buf,
            rect,
            ctx.all_frames,
            frames::FramesView {
                selected: ctx.hollywood.selected.as_ref(),
                active_studio: ctx.hollywood.active_studio.as_ref(),
                scroll: ctx.hollywood.frame_scroll,
                frame_h: ctx.plan.frames_h,
                mobile: ctx.opts.mobile,
                local: ctx.local,
                multi: ctx.multi,
                status: ctx.status,
                suppress_highlights: ctx.opts.suppress_highlights,
            },
            ctx.palette,
        );
        hits.drawn_studios = drawn
            .hits
            .iter()
            .filter(|(_, t)| matches!(t, Target::Studio(_)))
            .count();
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        hits.strip = false;
        y += drawn.height;
    } else if ctx.plan.frames_h == 1 {
        let rect = Rect::new(ctx.area.x, y, ctx.area.width, 1);
        let drawn = chips::render(
            buf,
            rect,
            ctx.all_frames,
            ctx.waits,
            ctx.status,
            ctx.hollywood.active_studio.as_ref(),
            ctx.hollywood.selected.as_ref(),
            ctx.hollywood.frame_scroll,
            ctx.studios_focused,
            ctx.local,
            ctx.multi,
            ctx.opts.suppress_highlights,
            ctx.palette,
        );
        if hits.studios_label.is_empty() {
            hits.studios_label = drawn.label_rect;
        }
        hits.rows.extend(drawn.hits);
        hits.drawn_studios = drawn.drawn_studios;
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        hits.strip = true;
        y += 1;
    }
    y
}
