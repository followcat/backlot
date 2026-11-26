//! Single-line compact studio rows (`Ready`, `Calm`, `Finished`,
//! `StageDown`, `Offline`) and the current studio's 1-line `CalmSummary`
//! row inside its bordered card.

use backlot_model::event::age_short;
use backlot_model::summary::{IDLE_SHOWN_MS, RoleSummary, StudioSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::data::{StudioLabel, StudioRef};
use super::draw_blocks::draw_inner;
use super::rows_fold::CompactKind;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::{Person, RoleBadge};

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

/// Build `(idle_count, standby_badges)` from calm/standby live roles omitted
/// from the current studio card while Left Cast is expanded.
pub(super) fn calm_summary_parts(roles: &[&RoleSummary]) -> Option<(usize, String)> {
    let mut idle_count = 0usize;
    let mut standby_badges = String::new();
    for role in roles {
        match Person::of(role) {
            Person::Idle | Person::Ready => idle_count += 1,
            Person::Standby => {
                if let Some(badge) = RoleBadge::of_role(&role.role) {
                    standby_badges.push_str(badge.glyph());
                } else {
                    standby_badges.push('·');
                }
            }
            _ => {}
        }
    }
    (idle_count > 0 || !standby_badges.is_empty()).then_some((idle_count, standby_badges))
}

/// Draw the 1-line calm/standby summary inside the current studio's bordered card:
/// `│ ○ 2 idle · Ⓦ standby │`.
pub(super) fn draw_calm_summary(
    buf: &mut Buffer,
    rect: Rect,
    idle_count: usize,
    standby_badges: &str,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let inner = rect.width.saturating_sub(5);
    if inner == 0 {
        return;
    }
    if idle_count > 0 {
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            1,
            "○",
            Style::default().fg(palette.overlay1),
        );
        let text = if standby_badges.is_empty() {
            format!("{idle_count} idle")
        } else {
            format!("{idle_count} idle · {standby_badges} standby")
        };
        let cut = truncate_end(&text, inner as usize);
        put_role_badged_text(
            buf,
            rect.x + 4,
            rect.y,
            inner,
            &cut,
            Style::default().fg(palette.overlay0),
            palette,
        );
    } else if !standby_badges.is_empty() {
        let text = truncate_end(&format!("{standby_badges} standby"), inner as usize);
        put_role_badged_text(
            buf,
            rect.x + 4,
            rect.y,
            inner,
            &text,
            Style::default().fg(palette.overlay0),
            palette,
        );
    }
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
    now_ms: u64,
    palette: &Palette,
) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    let (glyph, glyph_fg, name_fg) = match kind {
        CompactKind::Ready => ("✓", palette.teal, palette.subtext0),
        CompactKind::Calm => ("○", palette.overlay1, palette.subtext0),
        CompactKind::Finished => ("✓", palette.overlay0, palette.overlay0),
        CompactKind::StageDown => ("◌", palette.peach, palette.subtext0),
        CompactKind::Offline => ("×", palette.red, palette.subtext0),
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
    let max_label_w = right_limit
        .saturating_sub(label_x)
        .saturating_sub(tail_w + u16::from(tail_w > 0));
    if max_label_w > 0 {
        let fitted = label.fit(max_label_w as usize);
        put_text(
            buf,
            label_x,
            rect.y,
            max_label_w,
            &fitted,
            Style::default().fg(name_fg),
        );
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
