//! Pure helpers for the Design Reader variant (`ReaderKind::Design`):
//! Row 1/Row 2 formatting, semantic phase tokens, section navigation pills,
//! and action availability checks.

use super::reader_meta::{ReaderMeta, join_non_empty};
use crate::chrome::palette::Palette;
use backlot_model::design::{DesignPhase, SignMode};
use ratatui::style::Color;

/// Whether the design document is finalized/signed (`Signed` or `-final.md`).
pub(crate) fn design_finalized(m: &ReaderMeta) -> bool {
    m.design_phase == Some(DesignPhase::Signed)
        || m.file.as_deref().is_some_and(|f| f.ends_with("-final.md"))
}

/// Whether the design is waiting on the person's response (`WaitingUser` or `SuggestSkip`).
pub(crate) fn design_awaiting_user(m: &ReaderMeta) -> bool {
    !design_finalized(m)
        && matches!(
            m.design_phase,
            Some(DesignPhase::WaitingUser | DesignPhase::SuggestSkip)
        )
}

/// Row 1 title (`■ DESIGN v<N> · R<r> · <studio>`).
pub(super) fn row1_left(m: &ReaderMeta) -> String {
    let ver = m
        .design_version
        .or_else(|| m.file.as_deref().and_then(version_from_file));
    let head = match ver {
        Some(v) => format!("■ DESIGN v{v}"),
        None => "■ DESIGN".to_string(),
    };
    let round = m.design_round.filter(|r| *r > 0).map(|r| format!("R{r}"));
    join_non_empty([Some(head), round, m.studio.clone()])
}

/// Row 1 section indicator (`◀ <section> · i/N ▶` or `◀ i/N ▶`).
pub(super) fn nav_token(m: &ReaderMeta) -> Option<String> {
    let (Some(i), Some(n)) = (m.nav_index, m.nav_total) else {
        return None;
    };
    if n == 0 {
        return None;
    }
    match m.design_section.as_deref() {
        Some(sec) if !sec.is_empty() => Some(format!("◀ {sec} · {i}/{n} ▶")),
        _ => Some(format!("◀ {i}/{n} ▶")),
    }
}

/// Row 2 context (`[R<r>]`, semantic phase glyph + label, file path).
pub(super) fn row2(
    m: &ReaderMeta,
    palette: &Palette,
) -> (String, Option<(String, &'static str, Color)>, String) {
    let left = m
        .design_round
        .filter(|r| *r > 0)
        .map(|r| format!("R{r}"))
        .unwrap_or_default();
    let phase = m.design_phase.or_else(|| {
        m.file
            .as_deref()
            .is_some_and(|f| f.ends_with("-final.md"))
            .then_some(DesignPhase::Signed)
    });
    let state = phase.map(|p| {
        let (word, glyph, color) = phase_badge(p, m.design_sign_mode, palette);
        (word.to_string(), glyph, color)
    });
    let right = m.file.clone().unwrap_or_default();
    (left, state, right)
}

fn phase_badge(
    phase: DesignPhase,
    sign_mode: Option<SignMode>,
    palette: &Palette,
) -> (&'static str, &'static str, Color) {
    match phase {
        DesignPhase::WaitingProposal => ("proposal", "◆", palette.yellow),
        DesignPhase::WaitingBuildability => ("review", "◆", palette.blue),
        DesignPhase::WaitingSummary => ("summary", "◆", palette.mauve),
        DesignPhase::WaitingUser => ("waiting on you", "■", palette.peach),
        DesignPhase::SuggestSkip => ("skip suggested", "■", palette.yellow),
        DesignPhase::Signed => match sign_mode {
            Some(SignMode::Skipped) => ("skipped", "✓", palette.green),
            _ => ("signed", "✓", palette.green),
        },
        DesignPhase::Idle => ("idle", "□", palette.subtext0),
    }
}

/// Parse `v<N>` from `design/v<N>.md` or `design/v<N>-final.md`.
pub(crate) fn version_from_file(file: &str) -> Option<u32> {
    let name = file.rsplit('/').next()?;
    let stem = name.strip_suffix(".md")?;
    let ver = stem.strip_suffix("-final").unwrap_or(stem);
    ver.strip_prefix('v')?.parse::<u32>().ok()
}
