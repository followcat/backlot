//! Single-line compact studio rows (`Ready`, `Calm`, `Finished`,
//! `StageDown`, `Offline`).

use backlot_model::event::age_short;
use backlot_model::summary::{IDLE_SHOWN_MS, RoleSummary, StudioSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::data::{StudioLabel, StudioRef};
use super::rows_fold::CompactKind;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::studio::glyphs::{self, Person};

/// Whether a role demands attention or is currently active/queued (stays
/// expanded as its own 1-line row in non-current active cards and in
/// current cards even when Left Cast shows the full roster).
pub(super) fn is_active_or_alert_role(role: &RoleSummary) -> bool {
    matches!(
        Person::of(role),
        Person::UserPrompt
            | Person::DirPrompt
            | Person::NeedsYou
            | Person::Asks
            | Person::Stuck
            | Person::Failed
            | Person::Lost
            | Person::Working
            | Person::Slot
    )
}

/// Whether `studio` has any active/working/blocked/error signal that keeps
/// a non-current studio as a bordered card.
pub(super) fn has_active_or_alert(studio: &StudioSummary) -> bool {
    studio.error.is_some()
        || studio.needs_upgrade
        || !studio.outward_cards.is_empty()
        || super::draw_design::active_design(studio).is_some()
        || studio.roles.iter().any(is_active_or_alert_role)
}

/// Which 1-line compact category applies to `studio` when it is neither
/// current nor manually expanded nor carrying active/alert roles.
pub(super) fn compact_kind(
    studio: &StudioSummary,
    stage: bool,
    reachable: bool,
) -> Option<CompactKind> {
    if has_active_or_alert(studio) {
        return None;
    }
    if !reachable {
        return Some(CompactKind::Offline);
    }
    if !stage {
        return Some(CompactKind::StageDown);
    }
    if studio.ready_to_finish {
        return Some(CompactKind::Ready);
    }
    if studio.state == "finished" {
        return Some(CompactKind::Finished);
    }
    if studio.state == "live" {
        return Some(CompactKind::Calm);
    }
    None
}

fn calm_idle_age(studio: &StudioSummary, now_ms: u64) -> Option<String> {
    let latest = studio
        .roles
        .iter()
        .filter(|r| r.state == "live")
        .filter_map(|r| r.idle_since_ms)
        .filter(|&since| since > 0)
        .max()?;
    let elapsed = now_ms.saturating_sub(latest);
    (elapsed >= IDLE_SHOWN_MS).then(|| age_short(elapsed))
}

/// Draw a borderless 1-line compact studio row in `Expanded` mode:
/// ` ✓ gamma-web           5/5 ready [f] `
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_compact_studio(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    kind: CompactKind,
    label: &StudioLabel,
    selected: bool,
    focused_terminal: bool,
    now_ms: u64,
    palette: &Palette,
) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    let (glyph, glyph_fg) = match kind {
        CompactKind::Ready => ("✓", palette.teal),
        CompactKind::Calm => ("○", palette.overlay1),
        CompactKind::Finished => ("✓", palette.overlay0),
        CompactKind::StageDown => ("◌", palette.peach),
        CompactKind::Offline => ("×", palette.red),
    };
    put_text(
        buf,
        rect.x + 1,
        rect.y,
        1,
        glyph,
        Style::default().fg(glyph_fg),
    );

    let counts = (studio.studio.tasks_total > 0)
        .then(|| format!("{}/{}", studio.studio.tasks_done, studio.studio.tasks_total));
    let wide = rect.width >= 32;
    let right_limit = rect.right().saturating_sub(1);
    let mut tail_parts: Vec<(String, Style)> = Vec::new();
    match kind {
        CompactKind::Ready => {
            if let Some(c) = counts
                && rect.width >= 24
            {
                tail_parts.push((format!("{c} "), Style::default().fg(palette.overlay0)));
            }
            tail_parts.push(("ready".into(), Style::default().fg(palette.teal)));
            if wide {
                tail_parts.push((" [f]".into(), Style::default().fg(palette.subtext0)));
            }
        }
        CompactKind::Calm => {
            if let Some(c) = counts
                && rect.width >= 24
            {
                tail_parts.push((format!("{c} "), Style::default().fg(palette.overlay0)));
            }
            let word = match calm_idle_age(studio.studio, now_ms) {
                Some(age) => format!("idle {age}"),
                None => "idle".into(),
            };
            tail_parts.push((word, Style::default().fg(palette.overlay0)));
        }
        CompactKind::Finished => {
            tail_parts.push(("done".into(), Style::default().fg(palette.overlay0)));
            if rect.width >= 28 {
                tail_parts.push((" [o]".into(), Style::default().fg(palette.overlay0)));
            }
        }
        CompactKind::StageDown => {
            tail_parts.push(("stage down".into(), Style::default().fg(palette.peach)));
            if rect.width >= 30 {
                tail_parts.push((" [s]".into(), Style::default().fg(palette.subtext0)));
            }
        }
        CompactKind::Offline => {
            tail_parts.push(("offline".into(), Style::default().fg(palette.red)));
        }
    }

    let tail_w: u16 = tail_parts
        .iter()
        .map(|(t, _)| display_width(t) as u16)
        .sum();
    let label_x = rect.x + 3;
    let capsule = focused_terminal && !selected;
    let max_label_w = right_limit
        .saturating_sub(label_x)
        .saturating_sub(tail_w + u16::from(tail_w > 0) + u16::from(capsule && tail_w == 0));
    if max_label_w > 0 {
        let fitted = label.fit(max_label_w as usize);
        if capsule {
            let cap_style = glyphs::studio_capsule_style(palette);
            let drawn = display_width(&fitted) as u16;
            put_text(buf, label_x - 1, rect.y, 1, " ", cap_style);
            put_text(buf, label_x, rect.y, max_label_w, &fitted, cap_style);
            if drawn > 0 && label_x + drawn < right_limit {
                put_text(buf, label_x + drawn, rect.y, 1, " ", cap_style);
            }
        } else {
            put_text(
                buf,
                label_x,
                rect.y,
                max_label_w,
                &fitted,
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }

    let mut x = right_limit.saturating_sub(tail_w);
    if x >= label_x {
        for (text, style) in tail_parts {
            let w = display_width(&text) as u16;
            put_text(buf, x, rect.y, w, &text, style);
            x += w;
        }
    }
}
