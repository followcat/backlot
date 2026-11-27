//! Upper and waiting section painters for the expanded Call Sheet
//! band: top sprocket, title, studio frames/chips, waiting rule, and
//! the waiting-item scroll window.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::HollywoodRender;
use super::plan::Plan;
use super::rows::{Frame, HollywoodActivity, Row, Target};
use super::sheet::{Status, Wait};
use super::{Hollywood, LayerOpts, band, chips, frames, items, title};
use crate::chrome::palette::Palette;
use crate::panel::data::MachineSummary;

/// Shared inputs for the expanded band's section painters.
pub(super) struct BandCtx<'a> {
    pub frame: Rect,
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

/// Top sprocket, title row, studios rule, and the 7-row frames or
/// 1-row chip strip.
pub(super) fn draw_upper(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    let title_scroll = (!ctx.plan.waiting_rule && !ctx.plan.hints)
        .then_some(ctx.scroll_range)
        .flatten();
    if ctx.plan.top_sprocket {
        title::draw_sprocket(buf, ctx.area, y, '▄', ctx.palette);
        y += 1;
    }
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
            title_scroll,
            ctx.palette,
            hits,
        );
        y += 1;
    }
    if ctx.plan.blank_1 {
        y += 1;
    }
    if ctx.plan.studios_rule {
        super::zone::draw_studios_rule(
            buf,
            ctx.area,
            y,
            ctx.studios_n,
            ctx.studios_focused,
            ctx.palette,
        );
        y += 1;
    }
    if ctx.plan.frames_h == frames::FRAME_H {
        let rect = Rect::new(ctx.area.x, y, ctx.area.width, ctx.plan.frames_h);
        let drawn = frames::render(
            buf,
            rect,
            ctx.all_frames,
            frames::FramesView {
                selected: ctx.hollywood.selected.as_ref(),
                active_studio: ctx.hollywood.active_studio.as_ref(),
                scroll: ctx.hollywood.frame_scroll,
                mobile: ctx.opts.mobile,
                local: ctx.local,
                multi: ctx.multi,
                status: ctx.status,
                suppress_highlights: ctx.opts.suppress_highlights,
            },
            ctx.palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += drawn.height;
    } else if ctx.plan.frames_h == 1 {
        let rect = Rect::new(ctx.area.x, y, ctx.area.width, 1);
        let drawn = chips::render(
            buf,
            rect,
            ctx.all_frames,
            ctx.status,
            ctx.hollywood.selected.as_ref(),
            ctx.hollywood.frame_scroll,
            ctx.local,
            ctx.multi,
            ctx.opts.suppress_highlights,
            ctx.palette,
        );
        hits.rows.extend(drawn.hits);
        hits.more_left = drawn.more_left;
        hits.more_right = drawn.more_right;
        hits.body = rect;
        y += 1;
    }
    if ctx.plan.blank_2 {
        y += 1;
    }
    y
}

/// Upper-half lower zone: waiting rule, visible waiting items, or the
/// calm status line.
pub(super) fn draw_waiting(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    let user_urgent = items::any_user_urgent(ctx.waits);
    if ctx.plan.waiting_rule && !ctx.waits.is_empty() {
        super::zone::draw_waiting_rule(
            buf,
            ctx.area,
            y,
            ctx.waits.len(),
            user_urgent,
            ctx.wait_focused,
            ctx.scroll_range
                .map(|(top, bot)| (top, bot.min(ctx.waits.len()))),
            ctx.palette,
        );
        y += 1;
    }

    let mut wait_seen = 0usize;
    for row in ctx.band_rows {
        match row {
            Row::Wait(i) => {
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
                    ctx.palette,
                    hits,
                );
                y += h;
            }
            Row::Calm { .. } => {
                let rect = Rect::new(ctx.area.x, y, ctx.area.width, 1);
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
                    ctx.palette,
                    hits,
                );
                y += 1;
            }
            _ => {}
        }
    }
    y
}
