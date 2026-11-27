//! Middle HOLLYWOOD zone (`── HOLLYWOOD ──`, headline, talk rows,
//! live progress, compose/setup) and footer (`hints`, bottom `▀▀` edge).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::band::RowExtras;
use super::draw::{HollywoodRender, MARGIN};
use super::draw_sections::BandCtx;
use super::rows::{self, Row, Target};
use super::{band, hints, title};
use crate::chrome::scroll::list_scroll_metrics;

/// Middle zone: optional focused `── HOLLYWOOD ──` rule (with embedded
/// fold badge), headline, talk lines, live progress, and compose/setup.
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
    let total_earlier = model_fold.map_or(0, |(c, _)| c) + skipped_talks;
    let fold_badge = if total_earlier > 0 {
        let newest = if skipped_talks > 0 {
            talk_entries[skipped_talks - 1].seq
        } else {
            model_fold.map_or(0, |(_, n)| n)
        };
        Some(super::zone::FoldBadge {
            count: total_earlier,
            newest,
            selected: ctx.sel_ref == Some(&Target::Talk(newest)),
        })
    } else {
        None
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
                    RowExtras::default(),
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
                let talk_earlier = if !ctx.plan.hollywood_rule && idx + 1 == talk_total {
                    total_earlier
                } else {
                    0
                };
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
                        talk_earlier,
                        ..RowExtras::default()
                    },
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
                    RowExtras::default(),
                    ctx.palette,
                    hits,
                );
                y += 1;
            }
            Row::Input | Row::Setup if ctx.plan.tail_rows > 0 => {
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
                    RowExtras::default(),
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

/// Scroll metrics, hint bar, and bottom `▀▀` perforation edge (no external shadow).
pub(super) fn draw_footer(
    buf: &mut Buffer,
    mut y: u16,
    ctx: &BandCtx<'_>,
    hits: &mut HollywoodRender,
) {
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
        y += 1;
    }
    if ctx.plan.bottom_edge {
        title::draw_sprocket(buf, ctx.area, y, '▀', ctx.palette);
    }
}
