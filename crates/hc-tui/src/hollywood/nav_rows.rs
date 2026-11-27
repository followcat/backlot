//! Draw-aware vertical navigation stops across the Call Sheet's rows.

use backlot_model::summary::Exchange;

use super::rows::{Row, Target, band_rows_with_echoes};
use super::sheet::Status;
use crate::panel::data::MachineSummary;

/// Draw-aware vertical navigation stops across WAITING + HOLLYWOOD
/// for `↑`/`↓` movement so focus crosses zone boundaries cleanly without
/// landing on folded-off talk rows.
pub(crate) fn nav_band_targets(
    frame: ratatui::layout::Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &crate::hollywood::Hollywood,
    opts: crate::hollywood::LayerOpts,
) -> Vec<Target> {
    let (waits, rows) =
        band_rows_with_echoes(machines, status, opts.agent, &hollywood.reply_echoes);
    let mut nav_hw = crate::hollywood::Hollywood::new();
    nav_hw.mode = hollywood.mode;
    nav_hw.confirm = hollywood.confirm.clone();
    nav_hw.selected = hollywood.selected.clone();
    let plan = crate::hollywood::plan::plan(frame, &waits, &rows, &nav_hw, opts);
    let talk_total = rows.iter().filter(|r| matches!(r, Row::Talk(_))).count();
    let shown_talks = (plan.talk as usize).min(talk_total);
    let skipped_talks = talk_total.saturating_sub(shown_talks);
    let talk_entries: Vec<&Exchange> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => Some(*e),
            _ => None,
        })
        .collect();
    let model_fold = rows.iter().find_map(|r| match r {
        Row::Fold { count, newest } => Some((*count, *newest)),
        _ => None,
    });
    let fold_target = if plan.hollywood_rule && (skipped_talks > 0 || model_fold.is_some()) {
        if skipped_talks > 0 {
            Some(Target::Talk(talk_entries[skipped_talks - 1].seq))
        } else {
            model_fold.map(|(_, newest)| Target::Talk(newest))
        }
    } else {
        None
    };

    let mut out = Vec::new();
    let mut talk_seen = 0usize;
    let mut emitted_fold = false;
    for row in &rows {
        match row {
            Row::Wait(i) => out.push(waits[*i].target()),
            Row::Fold { .. } => {
                if !emitted_fold && let Some(t) = fold_target.clone() {
                    out.push(t);
                    emitted_fold = true;
                }
            }
            Row::Headline(_) if plan.headline => {
                if !emitted_fold && let Some(t) = fold_target.clone() {
                    out.push(t);
                    emitted_fold = true;
                }
                out.push(Target::Headline);
            }
            Row::Talk(line) => {
                if !emitted_fold && let Some(t) = fold_target.clone() {
                    out.push(t);
                    emitted_fold = true;
                }
                let idx = talk_seen;
                talk_seen += 1;
                if idx >= skipped_talks {
                    out.push(Target::Talk(line.seq));
                }
            }
            Row::Live(_) if plan.live => out.push(Target::HollywoodLive),
            Row::Input | Row::Setup => out.push(row.target(&waits)),
            _ => {}
        }
    }
    out
}
