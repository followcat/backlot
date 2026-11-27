//! Lower section and footer painters for the expanded Call Sheet
//! band: `HOLLYWOOD` rule, fold badge, headline, exchange lines, live
//! row, compose/setup rows, hints bar, bottom sprocket, and shadow.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::draw::{HollywoodRender, MARGIN};
use super::draw_sections::BandCtx;
use super::rows::{self, Row, Target};
use super::{band, hints, title};
use crate::chrome::render::put_text;
use crate::chrome::scroll::list_scroll_metrics;
use crate::chrome::text::display_width;

/// Lower zone: `HOLLYWOOD` rule (with embedded fold target) or setup
/// hairline, then headline, talk, live progress, compose, and setup.
pub(super) fn draw_lower(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) -> u16 {
    let talk_entries: Vec<&backlot_model::summary::Exchange> = ctx
        .band_rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => Some(*e),
            _ => None,
        })
        .collect();
    let talk_total = talk_entries.len();
    let shown_talks = (ctx.plan.talk as usize).min(talk_total);
    let skipped_talks = talk_total.saturating_sub(shown_talks);
    let model_fold = ctx.band_rows.iter().find_map(|r| match r {
        Row::Fold { count, newest } => Some((*count, *newest)),
        _ => None,
    });
    let fold_badge = if skipped_talks > 0 {
        let newest = talk_entries[skipped_talks - 1].seq;
        let count = model_fold.map_or(0, |(c, _)| c) + skipped_talks;
        Some(super::zone::FoldBadge {
            count,
            newest,
            selected: ctx.sel_ref == Some(&Target::Talk(newest)),
        })
    } else {
        model_fold.map(|(count, newest)| super::zone::FoldBadge {
            count,
            newest,
            selected: ctx.sel_ref == Some(&Target::Talk(newest)),
        })
    };

    if ctx.opts.agent && ctx.plan.hollywood_rule {
        super::zone::draw_hollywood_rule(
            buf,
            ctx.area,
            y,
            ctx.hollywood_focused,
            fold_badge,
            ctx.palette,
            hits,
        );
        y += 1;
    } else if !ctx.opts.agent && ctx.plan.setup_hairline {
        super::zone::draw_setup_hairline(buf, ctx.area, y, ctx.palette);
        y += 1;
    }

    let mut talk_seen = 0usize;
    for row in ctx.band_rows {
        match row {
            Row::Headline(_) if ctx.plan.headline => {
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
            Row::Talk(_) => {
                let idx = talk_seen;
                talk_seen += 1;
                if idx < skipped_talks {
                    continue;
                }
                let h = 1 + ctx.plan.talk_wrap_at(idx);
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
            Row::Live(_) if ctx.plan.live => {
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
            Row::Input | Row::Setup => {
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

/// Scroll metrics, hint bar, bottom sprocket, and external drop shadow.
pub(super) fn draw_footer(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) {
    let elev = ctx.palette.elevation();
    hits.metrics = Some(list_scroll_metrics(
        &vec![1; ctx.waits.len()],
        &[],
        ctx.plan.shown.max(1) as u16,
        ctx.scroll,
    ));
    if ctx.plan.hints {
        let live_prompt = rows::hollywood_asking(ctx.machines);
        let usable = (ctx.area.width as usize).saturating_sub((MARGIN * 2) as usize);
        let tier = if ctx.hollywood.confirm.is_some() {
            hints::pairs_for_confirm(usable)
        } else {
            hints::pairs_for_row_with_handoff(
                ctx.hollywood.selected.as_ref(),
                ctx.hollywood.editing,
                ctx.waits,
                ctx.opts.agent,
                live_prompt,
                usable,
            )
        };
        crate::chrome::render::put_hint_bar(
            buf,
            ctx.area.x + MARGIN,
            y,
            usable as u16,
            &tier,
            ctx.palette,
        );
        if !ctx.plan.waiting_rule
            && let Some((top, bot)) = ctx.scroll_range
        {
            let mark = format!("{top}–{bot}/{}", ctx.waits.len());
            let mw = display_width(&mark) as u16;
            let mx = (ctx.area.x + ctx.area.width).saturating_sub(MARGIN + mw);
            let hint_w = crate::chrome::render::hint_bar_width(&tier);
            if mx > ctx.area.x + MARGIN + hint_w + 1 {
                put_text(
                    buf,
                    mx,
                    y,
                    mw,
                    &mark,
                    Style::default().fg(elev.muted_surface).bg(elev.surface),
                );
            }
        }
        y += 1;
    }
    if ctx.plan.bottom_sprocket {
        title::draw_sprocket(buf, ctx.area, y, '▀', ctx.palette);
    }
    if ctx.plan.shadow {
        let shadow_row = Rect::new(ctx.area.x, ctx.area.bottom(), ctx.area.width, 1);
        crate::chrome::popup_frame::draw_bottom_shadow(
            buf,
            shadow_row,
            ctx.frame,
            ctx.palette,
            true,
        );
        hits.shadow_drawn = !shadow_row
            .intersection(ctx.frame)
            .intersection(buf.area)
            .is_empty();
    }
}
