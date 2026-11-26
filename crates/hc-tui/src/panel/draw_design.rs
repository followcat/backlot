//! Opening-design rendering and height accounting for the Lot Dock (`panel`)
//! and collapsed 16-col strip (`◆R2 ?` / `◆?`).

use backlot_model::design::DesignPhase;
use backlot_model::summary::{DesignSummary, StudioSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::data::StudioRef;
use super::draw_blocks::draw_inner;
use super::rows::Row;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;
use crate::studio::glyphs;

/// Active pre-signed design discussion for `studio` (excludes `Idle`, `Signed`,
/// `finished`, and `needs_upgrade` studios).
pub(crate) fn active_design(studio: &StudioSummary) -> Option<&DesignSummary> {
    if studio.needs_upgrade || studio.state == "finished" {
        return None;
    }
    studio
        .design
        .as_ref()
        .filter(|d| !matches!(d.phase, DesignPhase::Idle | DesignPhase::Signed))
}

/// Phase / turn word shown in `◆ design · … · <phase>`:
/// `you` on user turns, else `proposal`, `review`, or `summary`.
fn phase_word(design: &DesignSummary) -> &'static str {
    match design.phase {
        DesignPhase::SuggestSkip => "you",
        DesignPhase::WaitingUser => "you",
        DesignPhase::WaitingProposal => "proposal",
        DesignPhase::WaitingBuildability => "review",
        DesignPhase::WaitingSummary => "summary",
        DesignPhase::Idle | DesignPhase::Signed => "",
    }
}

/// `◆ design · R2 · you` / `◆ design · skip suggested · you` / `… · proposal|review|summary`.
pub(crate) fn header_text(design: &DesignSummary) -> String {
    let mid = if design.suggest_skip || design.phase == DesignPhase::SuggestSkip {
        "skip suggested".to_owned()
    } else {
        format!("R{}", design.round)
    };
    let turn = phase_word(design);
    if turn.is_empty() {
        format!("◆ design · {mid}")
    } else {
        format!("◆ design · {mid} · {turn}")
    }
}

/// Up to 3 question preview lines (or skip reason / summary when no questions).
pub(crate) fn preview_lines(design: &DesignSummary) -> Vec<String> {
    if design.phase == DesignPhase::SuggestSkip || design.suggest_skip {
        return design
            .skip_reason
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| vec![s.to_owned()])
            .unwrap_or_default();
    }
    if design.phase != DesignPhase::WaitingUser && !design.waiting_user {
        return Vec::new();
    }
    if !design.questions.is_empty() {
        return design
            .questions
            .iter()
            .take(3)
            .map(|q| format!("{}: {} → {}", q.id, q.prompt, q.recommendation))
            .collect();
    }
    design
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| vec![s.to_owned()])
        .unwrap_or_default()
}

pub(crate) fn has_actions(design: &DesignSummary) -> bool {
    design.waiting_user || design.phase.is_waiting_user()
}

/// Exact row count contributed to `block_rows` / `block_height`.
pub(crate) fn block_line_count(studio: &StudioSummary) -> usize {
    let Some(design) = active_design(studio) else {
        return 0;
    };
    1 + preview_lines(design).len() + usize::from(has_actions(design))
}

/// Exact row count contributed to `folded_block_rows` / `folded_block_height`.
pub(crate) fn folded_line_count(studio: &StudioSummary) -> usize {
    usize::from(active_design(studio).is_some())
}

/// Append active design rows inside an expanded (`Full`) studio block.
pub(crate) fn push_block_rows<'a>(out: &mut Vec<Row<'a>>, studio: StudioRef<'a>) {
    let Some(design) = active_design(studio.studio) else {
        return;
    };
    out.push(Row::DesignHeader { studio, design });
    for text in preview_lines(design) {
        out.push(Row::DesignPreview { studio, text });
    }
    if has_actions(design) {
        out.push(Row::DesignActions {
            studio,
            suggest_skip: design.suggest_skip || design.phase == DesignPhase::SuggestSkip,
        });
    }
}

/// Append single design summary line inside a `Folded` non-current studio block.
pub(crate) fn push_folded_rows<'a>(out: &mut Vec<Row<'a>>, studio: StudioRef<'a>) {
    let Some(design) = active_design(studio.studio) else {
        return;
    };
    out.push(Row::DesignHeader { studio, design });
}

/// Action footer text inside the Lot Dock design block.
pub(crate) fn actions_text(suggest_skip: bool) -> &'static str {
    if suggest_skip {
        "r discuss  o read  a skip"
    } else {
        "r reply   o read   a sign"
    }
}

/// Compact badge `(text, color)` for the 16-col collapsed strip (`◆R2 ?` / `◆?`).
pub(crate) fn strip_badge(
    studio: &StudioSummary,
    label_budget_with_badge: usize,
    palette: &Palette,
) -> Option<(String, Color)> {
    let design = active_design(studio)?;
    let user_turn = has_actions(design);
    let full = match (user_turn, design.round > 0) {
        (true, true) => format!("◆R{} ?", design.round),
        (true, false) => "◆?".to_owned(),
        (false, true) => format!("◆R{}", design.round),
        (false, false) => "◆".to_owned(),
    };
    let chosen = if user_turn && label_budget_with_badge < 2 {
        "◆?".to_owned()
    } else {
        full
    };
    Some((chosen, palette.peach))
}

fn inner_style(fg: Color, selected: bool, palette: &Palette) -> Style {
    let s = Style::default().fg(fg);
    if selected && let Some(bg) = glyphs::selection(palette).bg {
        return s.bg(bg);
    }
    s
}

/// Draw `│ ◆ design · … │` inside a studio block.
pub(crate) fn draw_header(
    buf: &mut Buffer,
    rect: Rect,
    design: &DesignSummary,
    selected: bool,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let inner = rect.width.saturating_sub(2);
    if inner == 0 {
        return;
    }
    let body_x = rect.x + 2;
    let width = inner.saturating_sub(2) as usize;
    let text = truncate_end(&header_text(design), width);
    let user_turn = has_actions(design);
    if let Some(rest) = text.strip_prefix("◆") {
        put_text(
            buf,
            body_x,
            rect.y,
            1,
            "◆",
            inner_style(palette.peach, selected, palette),
        );
        let rest_w = width.saturating_sub(1) as u16;
        if rest_w > 0 {
            let mut text_style = inner_style(
                if user_turn {
                    palette.peach
                } else {
                    palette.text
                },
                selected,
                palette,
            );
            if user_turn {
                text_style = text_style.add_modifier(Modifier::BOLD);
            }
            put_text(buf, body_x + 1, rect.y, rest_w, rest, text_style);
        }
    } else {
        let mut style = inner_style(
            if user_turn {
                palette.peach
            } else {
                palette.subtext0
            },
            selected,
            palette,
        );
        if user_turn {
            style = style.add_modifier(Modifier::BOLD);
        }
        put_text(buf, body_x, rect.y, width as u16, &text, style);
    }
}

/// Draw a truncated Q1..Q3 or summary preview line inside the block.
pub(crate) fn draw_preview(
    buf: &mut Buffer,
    rect: Rect,
    text: &str,
    selected: bool,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let inner = rect.width.saturating_sub(2);
    if inner <= 2 {
        return;
    }
    let style = inner_style(palette.subtext0, selected, palette);
    let width = inner.saturating_sub(3) as usize;
    put_text(
        buf,
        rect.x + 3,
        rect.y,
        width as u16,
        &truncate_end(text, width),
        style,
    );
}

/// Draw the `r reply   o read   a sign` action footer inside the block.
pub(crate) fn draw_actions(
    buf: &mut Buffer,
    rect: Rect,
    suggest_skip: bool,
    selected: bool,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let inner = rect.width.saturating_sub(2);
    if inner <= 2 {
        return;
    }
    let style = inner_style(palette.peach, selected, palette);
    let width = inner.saturating_sub(3) as usize;
    put_text(
        buf,
        rect.x + 3,
        rect.y,
        width as u16,
        &truncate_end(actions_text(suggest_skip), width),
        style,
    );
}
