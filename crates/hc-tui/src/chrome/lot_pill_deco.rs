//! Babylon Deco styling and `╡ … ╞` clasps for the top-bar Lot Pill (S4 §4).

use ratatui::style::Style;

use super::{LotCounts, PillTier, Seg};
use crate::chrome::palette::Palette;

pub(super) fn is_switch_urgent(counts: &LotCounts) -> bool {
    matches!(
        counts.switch_state,
        Some(
            crate::hollywood::PillSwitchState::NeedsApproval
                | crate::hollywood::PillSwitchState::QuotaOut { .. }
                | crate::hollywood::PillSwitchState::AuthError { .. }
        )
    )
}

/// Minimum width tier cap when clasps (`╡ ` + ` ╞`, +2 cols) are present.
pub(super) fn deco_cap(tier: PillTier) -> u16 {
    match tier {
        PillTier::Full => 48,
        PillTier::Wide => 30,
        PillTier::Compact | PillTier::Mobile => 18,
        PillTier::Tiny => 8,
    }
}

/// Replace leading `" "` and trailing `" "` padding with `╡ ` / ` ╞` clasps.
pub(super) fn apply_deco_clasps(
    segs: Vec<(String, Seg)>,
    counts: &LotCounts,
    spin: usize,
) -> Vec<(String, Seg)> {
    let switch_urgent = is_switch_urgent(counts);
    let blocking = counts.blocking > 0 || switch_urgent;
    let mut inner: Vec<(String, Seg)> = segs
        .into_iter()
        .map(|(text, seg)| {
            if blocking && seg == Seg::Spark {
                let normal_spark = if counts.activity.is_active() {
                    crate::studio::glyphs::Person::Working.glyph(spin)
                } else {
                    "✦"
                };
                (text.replace(normal_spark, "◉"), seg)
            } else {
                (text, seg)
            }
        })
        .collect();

    if let Some((first, _)) = inner.first_mut()
        && let Some(stripped) = first.strip_prefix(' ')
    {
        *first = stripped.to_string();
    }
    if inner.first().is_some_and(|(t, _)| t.is_empty()) {
        inner.remove(0);
    }

    if let Some((last, _)) = inner.last_mut()
        && let Some(stripped) = last.strip_suffix(' ')
    {
        *last = stripped.to_string();
    }
    if inner.last().is_some_and(|(t, _)| t.is_empty()) {
        inner.pop();
    }

    let mut out = Vec::with_capacity(inner.len() + 2);
    out.push(("╡ ".to_string(), Seg::Clasp));
    out.extend(inner);
    out.push((" ╞".to_string(), Seg::Clasp));
    out
}

/// Foreground/background style for calm pill clasps in Babylon studio mode.
pub(super) fn calm_clasp_style(palette: &Palette) -> Style {
    palette
        .elevation()
        .gilt_style(palette.elevation().raised_row)
}
